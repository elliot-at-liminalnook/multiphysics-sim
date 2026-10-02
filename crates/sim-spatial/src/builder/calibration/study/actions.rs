//! One typed apply owner for window, system_ui and REST offline study actions.
use super::{jobs, state::{DocumentCapture, StudyOwner, StudyStamp}};
use crate::{app::actions::{self, Act, Action, Call, InFlight, Replies, Spec}, document::DocumentRegistry};
use bevy::{ecs::message::Messages, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sim_runtime::experiment_study::commands::{self, Command, EvaluationSelection};
use sim_api::Outcome;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag="op",rename_all="snake_case",deny_unknown_fields)]
pub enum StudyAction {
    OpenArchive { path:String },
    OpenReview { path:String },
    Choose { id:u64 },
    Apply { stamp:StudyStamp, command:Command },
    Evaluate { stamp:StudyStamp, set:EvaluationSelection },
    Cancel { job:u64 },
    Save { stamp:StudyStamp, path:String },
    Export { stamp:StudyStamp, path:String },
    SystemUi { id:String, #[serde(default)] text:Option<String> },
    Status,
}
impl Action for StudyAction {
    fn commands()->Vec<Spec> { vec![actions::spec("system_measured_study",actions::BUILDER,json!({"op":"status"}),"Offline retained measured-PWM authoring. Typed operations open_archive/open_review/choose/apply/evaluate/cancel/save/export/status. Apply commands use shared experiment_study validation; all scoped operations require stamp {id,revision}. Publication creates new destinations only; accepted jobs return an id and retain receipts.")] }
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
    actions::apply(&mut messages,&mut inflight,&mut replies,|action,call| {
        if let StudyAction::SystemUi{id,text}=action {
            if call.cancelled { return Outcome::Done(Err("Study control activation cancelled".into())); }
            return match super::forms::activate(&ui,id) {
                Ok(hit) if text.is_some()=>match super::forms::text_submission(&mut ui,&owner,hit,text.as_deref().unwrap_or_default()) {
                    Ok(action)=>{
                        let outcome=handle(&mut owner,&registry,&action,call);
                        super::forms::acknowledge(&mut ui,&action,&outcome);
                        outcome
                    }
                    Err(e)=>{ui.error=Some(e.clone());Outcome::Done(Err(e))},
                },
                Ok(super::forms::Hit::Action(action))=>{
                    let outcome=handle(&mut owner,&registry,&action,call);
                    super::forms::acknowledge(&mut ui,&action,&outcome);
                    outcome
                },
                Ok(hit)=>{ui.pending.push(hit);ui.epoch+=1;Outcome::Done(Ok(json!({"message":"Study form control activated"})))},
                Err(e)=>{owner.status=e.clone();Outcome::Done(Err(e))},
            };
        }
        let outcome=handle(&mut owner,&registry,action,call);
        super::forms::acknowledge(&mut ui,action,&outcome);
        outcome
    });
}
pub fn handle(owner:&mut StudyOwner, registry:&DocumentRegistry, action:&StudyAction, call:&mut Call)->Outcome {
    if call.cancelled { return Outcome::Done(Err("Measured study request cancelled before application; retained work is unchanged".into())); }
    if matches!(action,StudyAction::Status) { return Outcome::Done(apply_action(owner,registry,action)); }
    let result=apply_action(owner,registry,action);
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
    match action {
        StudyAction::Status=>{
            // Full status includes traces from every retained archive, irrespective
            // of its current filter. Record that held-out exposure before returning.
            for retained in &mut owner.studies {
                let seen=retained.study.validation_seen;
                let ids=retained.study.archive.trials.iter().map(|t|t.id.clone()).collect::<Vec<_>>();
                commands::expose(&mut retained.study,&ids);
                if retained.study.validation_seen!=seen {retained.revision+=1;owner.changed+=1;}
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
            let seen=s.study.validation_seen;
            let ids=commands::filtered_ids(&s.study);
            commands::expose(&mut s.study,&ids);
            if s.study.validation_seen!=seen { s.revision+=1; }
            owner.active=Some(*id);owner.selection_epoch+=1;
        }
        StudyAction::Apply{stamp,command}=>{
            checked(owner,registry,*stamp)?;
            let retained=owner.get_mut(stamp.id).ok_or("study.id: missing retained study")?;
            commands::apply(&mut retained.study,command.clone())?;
            retained.revision+=1;
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
            let id=jobs::start_publication(owner,*stamp,path,export)?;
            return Ok(json!({"job":id,"message":"Publishing a new immutable captured review revision"}));
        }
        StudyAction::SystemUi{..}=>return Err("study.system_ui: rendered controls must be resolved by the apply owner".into()),
    }
    Ok(json!({"message":"Retained measured study updated","active":owner.active,"stamp":owner.active().map(|s|s.stamp())}))
}

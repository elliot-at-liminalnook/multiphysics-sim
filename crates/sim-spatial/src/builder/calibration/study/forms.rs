//! Kit text focus and retained form buffers. These are presentation inputs,
//! never study truth. Every submission carries the identity displayed at focus.
use super::{StudyAction, StudyOwner, StudyStamp};
use bevy::prelude::*;
use crate::{app::actions::Act, builder::ui_api::Enabled, ui_kit::text::{FieldEvent, FieldId, FieldMsg, TextFocus}};
use sim_runtime::experiment_study::commands::{Command, ParameterGroup};
use std::collections::BTreeMap;

pub(crate) const FIELD: FieldId = FieldId("build.measured-study");
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Field {
    Archive, Review, Save, Export, Parameter(bool, String), Condition(&'static str), Step,
    Limit(&'static str), Filter(&'static str), Notes, DecisionNotes(usize),
}
#[derive(Component, Clone, Debug)]
pub(crate) enum Hit {
    Action(StudyAction),
    Focus { stamp: Option<StudyStamp>, field: Field, text: String },
    Path { stamp: Option<StudyStamp>, field: Field, text: String },
    Discard { key: DraftKey },
    SelectedComponent { stamp:StudyStamp },
}
pub(crate) type DraftKey=(Option<(u64,u64)>,Field);
#[derive(Resource)]
pub(crate) struct StudyUi {
    pub(crate) paths: BTreeMap<Field, String>,
    pub(crate) focus: Option<(Option<StudyStamp>, Field)>,
    pub(crate) buffer: String,
    pub(crate) error: Option<String>,
    pub(crate) chart: super::chart::Chart,
    pub(crate) epoch: u64,
    pub(crate) rendered: BTreeMap<String, super::ui::RenderedControl>,
    pub(crate) pending: Vec<Hit>,
    pub(crate) drafts: BTreeMap<DraftKey,String>,
    pub(crate) awaiting: BTreeMap<String,(DraftKey,String)>,
    pub(crate) blur_requested: bool,
}
impl Default for StudyUi {
    fn default()->Self {
        let archive=crate::workspace::path(super::super::DEFAULT_ARCHIVE).map(|p|p.display().to_string()).unwrap_or_else(|_|super::super::DEFAULT_ARCHIVE.into());
        Self {paths:BTreeMap::from([(Field::Archive,archive)]),focus:None,buffer:String::new(),error:None,chart:Default::default(),epoch:0,rendered:BTreeMap::new(),pending:Vec::new(),drafts:BTreeMap::new(),awaiting:BTreeMap::new(),blur_requested:false}
    }
}
pub(crate) fn activate(ui: &StudyUi, id: &str) -> Result<Hit, String> {
    let control=ui.rendered.get(id).ok_or_else(||format!("{id}: no rendered study control"))?;
    if !control.enabled {return Err(format!("{id}: the rendered control is disabled"));}
    Ok(control.hit.clone())
}
impl StudyUi {
    pub(crate) fn blocking_reason(&self)->Option<String> {
        (!self.drafts.is_empty() || !self.awaiting.is_empty() || !self.pending.is_empty() || self.focus.is_some()).then(||"Measured study fields have unresolved input. Submit the retained field drafts or explicitly discard them before leaving.".into())
    }
    pub(crate) fn shown(&self, stamp: Option<StudyStamp>, field: &Field, fallback: String) -> String {
        if self.focus.as_ref().is_some_and(|(s,f)| *s == stamp && f == field) { self.buffer.clone() }
        else { self.drafts.get(&(stamp.map(|s|(s.id,s.revision)),field.clone())).cloned().or_else(||self.paths.get(field).cloned()).unwrap_or(fallback) }
    }
    pub(crate) fn focused(&self, stamp: Option<StudyStamp>, field: &Field) -> bool {
        self.focus.as_ref().is_some_and(|(s,f)| *s == stamp && f == field)
    }
}
fn draft_key(stamp:Option<StudyStamp>,field:Field)->DraftKey { (stamp.map(|s|(s.id,s.revision)),field) }
fn stage(ui:&mut StudyUi,owner:&StudyOwner,stamp:Option<StudyStamp>,field:Field,text:&str)->Result<StudyAction,String> {
    let key=draft_key(stamp,field.clone());
    if ui.awaiting.values().any(|(k,_)|k==&key) {return Err("Measured field submission is awaiting its action acknowledgment".into());}
    ui.drafts.insert(key.clone(),text.into());
    if matches!(field,Field::Archive|Field::Review|Field::Save|Field::Export){ui.paths.insert(field.clone(),text.into());}
    let action=submission(owner,stamp,&field,text)?;
    let fingerprint=serde_json::to_string(&action).map_err(|e|e.to_string())?;
    if ui.awaiting.contains_key(&fingerprint){return Err("An equivalent measured field action already awaits acknowledgment; both raw field drafts are retained".into());}
    ui.awaiting.insert(fingerprint,(key,text.into()));
    Ok(action)
}
pub(crate) fn text_submission(ui:&mut StudyUi,owner:&StudyOwner,hit:Hit,text:&str)->Result<StudyAction,String> {
    match hit {
        Hit::Focus{stamp,field,..}|Hit::Path{stamp,field,..}=>stage(ui,owner,stamp,field,text),
        _=>Err("This study control does not accept text".into()),
    }
}
/// Called only after the one StudyAction owner has actually accepted/refused
/// the request. Parsing or enqueueing cannot erase a rejected field draft.
pub(crate) fn acknowledge(ui:&mut StudyUi,action:&StudyAction,outcome:&sim_api::Outcome) {
    let Ok(fingerprint)=serde_json::to_string(action) else{return};
    let sim_api::Outcome::Done(result)=outcome else{return};
    let Some((key,text))=ui.awaiting.remove(&fingerprint) else{return};
    match result {
        Ok(_)=> {
            if ui.drafts.get(&key)==Some(&text) {
                ui.drafts.remove(&key);
                if ui.focus.clone().map(|(stamp,field)|draft_key(stamp,field))==Some(key) {ui.focus=None;ui.blur_requested=true;}
            }
            ui.error=None;
        }
        Err(e)=>ui.error=Some(e.clone()),
    }
    ui.epoch+=1;
}
fn number(path: &str, text: &str) -> Result<f64, String> {
    let value = text.trim().parse::<f64>().map_err(|e| format!("{path}: {e}"))?;
    if !value.is_finite() { return Err(format!("{path}: must be finite")); }
    Ok(value)
}
fn optional(path: &str, text: &str) -> Result<Option<f64>, String> {
    if text.trim().is_empty() { Ok(None) } else { number(path, text).map(Some) }
}
/// Parsing is deliberately small; all model/selection validation remains shared.
pub(crate) fn submission(owner: &StudyOwner, stamp: Option<StudyStamp>, field: &Field, text: &str) -> Result<StudyAction,String> {
    let path = || crate::ui_kit::path_field::expand(text.trim());
    match field {
        Field::Archive => return Ok(StudyAction::OpenArchive { path: path() }),
        Field::Review => return Ok(StudyAction::OpenReview { path: path() }),
        _ => {}
    }
    let stamp = stamp.ok_or("No retained study is selected")?;
    let retained = owner.active().filter(|r| r.stamp() == stamp).ok_or("The displayed study was replaced or edited; text is retained. Focus the field again to edit the current revision.")?;
    match field {
        Field::Save => return Ok(StudyAction::Save {stamp,path:path()}),
        Field::Export => return Ok(StudyAction::Export {stamp,path:path()}),
        _ => {}
    }
    let study = &retained.study;
    let command = match field {
        Field::Parameter(bridge,name) => Command::SetParameter { group: if *bridge { ParameterGroup::Bridge } else { ParameterGroup::Motor }, name:name.clone(), value:number(name,text)? },
        Field::Step => Command::SetStep(number("draft.step_s",text)?),
        Field::Condition(name) => {
            let mut c = study.draft.conditions.clone();
            match *name {
                "voltage_v" => c.voltage_v=optional("draft.conditions.voltage_v",text)?,
                "temperature_c" => c.temperature_c=optional("draft.conditions.temperature_c",text)?,
                "load_inertia" => c.load_inertia=number("draft.conditions.load_inertia",text)?,
                "load_torque" => c.load_torque=number("draft.conditions.load_torque",text)?,
                "command_delay_s" => c.command_delay_s=number("draft.conditions.command_delay_s",text)?,
                _ => return Err(format!("conditions.{name}: unknown field")),
            }
            Command::SetConditions(c)
        }
        Field::Limit(name) => {
            let mut limits = study.limits.clone().or_else(|| study.archive.trials.first().map(|t| t.limits.clone())).ok_or("limits: no trial limits are available")?;
            match *name { "rmse" => limits.rmse=number("limits.rmse",text)?, "final_abs_error"=>limits.final_abs_error=number("limits.final_abs_error",text)?, _=>return Err("limits: unknown field".into()) }
            Command::SetLimits(Some(limits))
        }
        Field::Filter(name) => {
            let mut v = study.view.clone();
            match *name {
                "device" => { let n=number("view.device",text)?; if n.fract()!=0. || !(0. ..=255.).contains(&n) {return Err("view.device: expected an integer 0–255".into())} v.device=n as u8; }
                "direction" => { let n=number("view.direction",text)?; if n.fract()!=0. || !(-1. ..=1.).contains(&n) {return Err("view.direction: expected -1, 0 or 1".into())} v.direction=n as i8; }
                "min_drive" => v.min_drive=number("view.min_drive",text)?,
                "max_drive" => v.max_drive=number("view.max_drive",text)?,
                _=>return Err("view: unknown filter".into()),
            }
            Command::SetView(v)
        }
        Field::Notes => Command::SetNotes(text.into()),
        Field::DecisionNotes(index) => {
            let evaluation=study.evaluations.get(*index).ok_or("evaluation: no longer present")?;
            Command::SetDecision{evaluation:*index,decision:evaluation.decision.clone(),notes:text.into()}
        }
        _ => return Err("Unsupported study field".into()),
    };
    Ok(StudyAction::Apply { stamp, command })
}
/// Input occurrences become typed actions. Process old text occurrences before
/// new clicks, so a click cannot retarget an already queued submission.
pub(crate) fn input(
    mut fields: MessageReader<FieldMsg>,
    buttons: Query<(&Interaction,&Hit,Option<&Enabled>), (Changed<Interaction>,With<Button>)>,
    owner: Res<StudyOwner>, mut ui: ResMut<StudyUi>, mut focus: TextFocus,
    mut actions: MessageWriter<Act<StudyAction>>, mut builder: Option<ResMut<crate::builder::Builder>>,
    selection: Option<Res<crate::selection::Selection>>, registry:Option<Res<crate::document::DocumentRegistry>>,
) {
    if ui.blur_requested {focus.blur(FIELD);ui.blur_requested=false;}
    for msg in fields.read().filter(|m|m.field==FIELD) {
        match &msg.event {
            FieldEvent::Changed(draft) => {
                let Some((stamp,field))=ui.focus.clone() else {continue};
                let key=(stamp.map(|s|(s.id,s.revision)),field);
                // Caret/selection occurrences from the kit do not create an
                // unsent study edit when the working text is unchanged.
                if ui.buffer==draft.text {continue}
                ui.buffer=draft.text.clone();
                ui.drafts.insert(key,draft.text.clone());
                ui.epoch+=1; if let Some(builder)=builder.as_mut(){builder.panel_dirty=true;}
            }
            FieldEvent::Submit(text) => {
                if let Some((stamp,field))=ui.focus.clone() {
                    match stage(&mut ui,&owner,stamp,field,text) {
                        Ok(action)=> {actions.write(Act::ui(action)); ui.error=None;}
                        Err(e)=>ui.error=Some(e),
                    }
                    ui.epoch+=1; if let Some(builder)=builder.as_mut(){builder.panel_dirty=true;}
                }
            }
            FieldEvent::Cancel | FieldEvent::Blur if ui.focus.is_some() => { ui.focus=None; ui.epoch+=1; if let Some(builder)=builder.as_mut(){builder.panel_dirty=true;} }
            _=>{}
        }
    }
    let mut hits=if ui.pending.is_empty(){Vec::new()}else{std::mem::take(&mut ui.pending)};
    hits.extend(buttons.iter().filter(|(interaction,_,enabled)|**interaction==Interaction::Pressed && !enabled.is_some_and(|e|!e.0)).map(|(_,hit,_)|hit.clone()));
    for hit in hits {
        match &hit {
            Hit::Action(action)=>{actions.write(Act::ui(action.clone()));}
            Hit::Focus{stamp,field,text}=> {
                ui.focus=Some((*stamp,field.clone())); ui.buffer=text.clone(); ui.error=None;
                focus.focus(FIELD,text.clone()); ui.epoch+=1; if let Some(builder)=builder.as_mut(){builder.panel_dirty=true;}
            }
            Hit::Path{stamp,field,text}=> {
                let text=ui.shown(*stamp,field,text.clone());
                match stage(&mut ui,&owner,*stamp,field.clone(),&text) {Ok(a)=>{actions.write(Act::ui(a));ui.error=None},Err(e)=>ui.error=Some(e)}
                ui.epoch+=1; if let Some(builder)=builder.as_mut(){builder.panel_dirty=true;}
            }
            Hit::Discard{key}=> {
                if ui.awaiting.values().any(|(k,_)|k==key){ui.error=Some("Await action acknowledgment before discarding this field".into());continue}
                ui.drafts.remove(key);
                if ui.focus.clone().map(|(stamp,field)|draft_key(stamp,field))==Some(key.clone()){ui.focus=None;focus.blur(FIELD);}
                ui.error=None;ui.epoch+=1;if let Some(builder)=builder.as_mut(){builder.panel_dirty=true;}
            }
            Hit::SelectedComponent{stamp}=> {
                let result=(|| {
                    let retained=owner.validate_stamp(*stamp)?;
                    let document=registry.as_ref().and_then(|r|r.entry(crate::app::ViewerMode::Build)).ok_or("No Build document is available for the shared selection")?;
                    let ids=selection.as_ref().map(|s|s.components(document.id)).unwrap_or_default();
                    if ids.len()!=1{return Err("Select exactly one Build component before linking its stable ID to this review".into());}
                    let mut view=retained.study.view.clone();view.component_id=Some(ids[0].clone());
                    Ok(StudyAction::Apply{stamp:*stamp,command:Command::SetView(view)})
                })();
                match result {Ok(action)=>{actions.write(Act::ui(action));ui.error=None},Err(e)=>ui.error=Some(e)}
                ui.epoch+=1;if let Some(builder)=builder.as_mut(){builder.panel_dirty=true;}
            }
        }
    }
}

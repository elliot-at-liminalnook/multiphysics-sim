//! Sole ordinary-close owner. Messages are intent occurrences; pending intent
//! and preference-loss scope survive in this resource. No authored work lives
//! in the window markers or presentation entities.
use super::{ViewerSet, actions::{self, Act, Action, InFlight, Replies, Spec}, settings::SettingsOwner};
use crate::builder::calibration::study::{StudyOwner, forms::StudyUi};
use bevy::{ecs::message::Messages, prelude::*, window::{ClosingWindow, WindowCloseRequested}};
use serde::{Deserialize, Serialize};
use sim_api::Outcome;
pub(crate) mod ui;
#[cfg(test)] mod tests;

#[derive(Component, Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag="command", rename_all="snake_case")]
pub enum CloseAction { CloseRequest, CloseRetry, CloseCancel, CloseWithoutPreferences, CloseStatus }
impl Action for CloseAction {
    fn commands()->Vec<Spec> {
        [("close_request","Request graceful closure after preserving authored work and publishing preferences"),
         ("close_retry","Retry unresolved preference persistence during closure"),
         ("close_cancel","Cancel close intent while retaining drafts and preference jobs"),
         ("close_without_preferences","Acknowledge loss of currently unresolved preferences only"),
         ("close_status","Inspect native close lifecycle and preservation blockers")]
        .into_iter().map(|(id,description)|actions::spec(id,actions::ALL,serde_json::json!({}),description)).collect()
    }
    fn parse(command:&sim_api::Command)->Result<Self,String> {
        if command.command=="system_ui" {
            let action=command.args.get("action").ok_or("system_ui.action is required")?;
            if action.get("operation").and_then(serde_json::Value::as_str)!=Some("activate") {
                return Err("close controls require operation activate".into());
            }
            return match action.get("id").and_then(serde_json::Value::as_str) {
                Some("close:request")=>Ok(Self::CloseRequest), Some("close:retry")=>Ok(Self::CloseRetry),
                Some("close:cancel")=>Ok(Self::CloseCancel), Some("close:without_preferences")=>Ok(Self::CloseWithoutPreferences),
                Some("close:status")=>Ok(Self::CloseStatus), _=>Err("unknown close control".into()),
            };
        }
        sim_api::decode(command)
    }
    fn controls()->&'static [&'static str] { &["close:request","close:retry","close:cancel","close:without_preferences","close:status"] }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CloseSnapshot {
    pub pending: bool, pub status: String, pub blockers: Vec<String>,
    pub request_enabled: bool, pub retry_enabled: bool, pub cancel_enabled: bool,
    pub preference_exit_enabled: bool,
    pub preference_stamp: u64, pub preference_loss_scope: Option<u64>,
}
#[derive(Resource, Default)]
pub struct CloseOwner {
    pending: bool,
    bypass: Option<u64>,
    armed: Option<u64>,
    snapshot: CloseSnapshot,
    refusal: Option<String>,
}
impl CloseOwner {
    pub fn snapshot(&self)->&CloseSnapshot { &self.snapshot }
    pub fn pending(&self)->bool { self.pending }
}
/// Public feature ordering; hardware actions precede close STOP application.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CloseSet { Apply, Publish }

pub struct ClosePlugin;
impl Plugin for ClosePlugin {
    fn build(&self,app:&mut App) {
        actions::register::<CloseAction>(app);
        app.init_resource::<CloseOwner>()
            .add_systems(Update,window_input.in_set(ViewerSet::Input))
            .add_systems(Update,apply.in_set(ViewerSet::Actions).in_set(CloseSet::Apply))
            .add_systems(Update,present.in_set(ViewerSet::Present).in_set(CloseSet::Publish))
            .add_systems(Last,authorize.before(bevy::window::ExitSystems));
        ui::register(app);
    }
}
fn window_input(mut input:MessageReader<WindowCloseRequested>,mut output:MessageWriter<Act<CloseAction>>) {
    if input.read().count()>0 { output.write(Act::quiet(CloseAction::CloseRequest)); }
}
/// Feature facts remain owned by their features. CAD's release owner detaches
/// potentially unsaved services on exit; it is not permission to discard them.
fn blockers(studies:Option<&StudyOwner>,ui:Option<&StudyUi>)->Vec<String> {
    ui.and_then(StudyUi::blocking_reason).into_iter()
        .chain(studies.and_then(StudyOwner::blocking_reason)).collect()
}
fn project(owner:&CloseOwner,settings:&SettingsOwner,blockers:Vec<String>)->CloseSnapshot {
    let pending=owner.pending;
    let preference_ready=settings.drain_ready() || owner.bypass==Some(settings.drain_stamp());
    let status=if !blockers.is_empty() { format!("Close refused: {}",blockers.join(" ")) }
        else if pending && preference_ready {
            if owner.bypass==Some(settings.drain_stamp()) && !settings.drain_ready() {
                format!("Closing: preference publication skipped by explicit acknowledgment for scope {}",settings.drain_stamp())
            } else { "Closing: latest preference publication acknowledged".into() }
        }
        else if pending { format!("Waiting for preferences: {:?}",settings.drain_status()) }
        else { owner.refusal.clone().unwrap_or_else(||"Window open".into()) };
    CloseSnapshot {pending,status,request_enabled:!pending && blockers.is_empty(),
        retry_enabled:pending && !settings.drain_ready(),cancel_enabled:pending,
        preference_exit_enabled:pending && blockers.is_empty() && !settings.drain_ready(),
        preference_stamp:settings.drain_stamp(),preference_loss_scope:owner.bypass,blockers}
}
fn apply(mut messages:ResMut<Messages<Act<CloseAction>>>,mut flight:ResMut<InFlight<CloseAction>>,
    mut replies:ResMut<Replies>,mut owner:ResMut<CloseOwner>,mut settings:ResMut<SettingsOwner>,
    studies:Option<Res<StudyOwner>>,ui:Option<Res<StudyUi>>,
    mut hardware:Option<ResMut<crate::robot::hardware::Hardware>>) {
    actions::apply(&mut messages,&mut flight,&mut replies,|action,_| {
        // STOP precedes even preservation refusal and any preference wait.
        if matches!(action,CloseAction::CloseRequest|CloseAction::CloseWithoutPreferences) {
            if let Some(hw)=hardware.as_deref_mut() { crate::robot::hardware::actions::request_close_stop(hw); }
        }
        let facts=blockers(studies.as_deref(),ui.as_deref());
        let projection=project(&owner,&settings,facts.clone());
        let result=match action {
            CloseAction::CloseStatus=>Ok(()),
            CloseAction::CloseCancel if projection.cancel_enabled=>{owner.pending=false;owner.bypass=None;owner.armed=None;owner.refusal=None;Ok(())},
            CloseAction::CloseRetry if projection.retry_enabled=>{settings.retry();Ok(())},
            CloseAction::CloseWithoutPreferences if projection.preference_exit_enabled=>{owner.bypass=Some(settings.drain_stamp());owner.armed=None;Ok(())},
            CloseAction::CloseRequest if projection.request_enabled=>{owner.pending=true;owner.refusal=None;Ok(())},
            _=>Err(if !facts.is_empty(){format!("Close refused: {}",facts.join(" "))}else{"Close action is disabled in the current lifecycle state".into()}),
        };
        if let Err(reason)=&result { owner.refusal=Some(reason.clone()); }
        let snapshot=project(&owner,&settings,facts);
        owner.snapshot=snapshot;
        Outcome::Done(result.map(|()|serde_json::to_value(owner.snapshot()).unwrap_or_default()))
    });
}
fn present(mut owner:ResMut<CloseOwner>,settings:Res<SettingsOwner>,studies:Option<Res<StudyOwner>>,ui:Option<Res<StudyUi>>) {
    let next=project(&owner,&settings,blockers(studies.as_deref(),ui.as_deref()));
    if owner.snapshot!=next { owner.snapshot=next; }
}
/// Final public Last boundary, before ExitSystems: re-read current settings and
/// preservation after every Update producer. Never authorize from Present's
/// cached projection. Two frames preserve Bevy's ClosingWindow loss boundary.
fn authorize(mut commands:Commands,mut owner:ResMut<CloseOwner>,settings:Res<SettingsOwner>,
    studies:Option<Res<StudyOwner>>,ui:Option<Res<StudyUi>>,
    windows:Query<(Entity,Option<&ClosingWindow>),With<Window>>) {
    let facts=blockers(studies.as_deref(),ui.as_deref());
    let stamp=settings.drain_stamp();
    let valid=owner.pending && facts.is_empty() && (settings.drain_ready() || owner.bypass==Some(stamp));
    if owner.bypass.is_some_and(|ack|ack!=stamp) { owner.bypass=None; }
    if !valid {
        if owner.armed.is_some() { owner.armed=None; }
        for (entity,closing) in &windows { if closing.is_some(){commands.entity(entity).remove::<ClosingWindow>();} }
    } else if owner.armed==Some(stamp) {
        for (entity,_) in &windows { commands.entity(entity).despawn(); }
    } else {
        owner.armed=Some(stamp);
        for (entity,_) in &windows { commands.entity(entity).try_insert(ClosingWindow); }
    }
    let next=project(&owner,&settings,facts);
    if owner.snapshot!=next { owner.snapshot=next; }
}

//! Transactional offline authoring. Hosts retain identity/revision and schedule runtime jobs.
use super::{Conditions, ModelSettings, ReviewView, Study};
use crate::experiment_comparison::Limits;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParameterGroup { Motor, Bridge }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EvaluationSelection { Selected, Filtered, HeldOut, Ids(Vec<String>) }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    SetCandidate(ModelSettings),
    SetParameter { group: ParameterGroup, name: String, value: f64 },
    SetConditions(Conditions), SetStep(f64), SetLimits(Option<Limits>),
    SetView(ReviewView), SelectTrial(Option<String>), SelectEvaluation(Option<usize>),
    SetNotes(String), SetDecision { evaluation: usize, decision: String, notes: String },
    ResetCandidate, UseEvaluation(usize), Expose(EvaluationSelection),
}
#[derive(Clone, Debug)]
pub struct ParameterInfo {
    pub group: ParameterGroup, pub name: String, pub unit: String,
    pub default: Option<f64>, pub minimum: Option<f64>, pub maximum: Option<f64>,
    pub exclusive_minimum: bool, pub integer: bool,
}
pub fn metadata() -> Vec<ParameterInfo> {
    let registry = crate::registry();
    let mut rows = Vec::new();
    for (group, kind) in [(ParameterGroup::Motor, sim_domain_robot::motor::MOTOR_UNIT), (ParameterGroup::Bridge, sim_domain_robot::motor::H_BRIDGE)] {
        if let Ok(descriptor) = registry.get(&kind.into()) {
            for p in descriptor.parameters.as_ref().into_iter().flatten() {
                if p.implementation_reference || p.name.starts_with("initial.") || p.name.contains('*') { continue; }
                rows.push(ParameterInfo { group, name:p.name.clone(), unit:p.unit.clone(), default:p.default,
                    minimum:p.minimum, maximum:p.maximum, exclusive_minimum:p.exclusive_minimum, integer:p.integer });
            }
        }
    }
    rows
}
pub const DECISIONS: &[&str] = &["Investigating", "Retain baseline", "Rejected candidate", "Preferred for tested conditions"];
pub fn validate_limits(limits: &Option<Limits>) -> Result<(), String> {
    if let Some(l) = limits {
        for (path, value) in [("limits.rmse",l.rmse),("limits.final_abs_error",l.final_abs_error)] {
            if !value.is_finite() || value < 0. { return Err(format!("{path}: must be finite and nonnegative")); }
        }
    }
    Ok(())
}
pub fn validate_view(s: &Study) -> Result<(), String> {
    let v=&s.view;
    if v.evaluation.is_some_and(|i|i>=s.evaluations.len()) { return Err("view.evaluation: unknown evaluation".into()); }
    if v.trial_id.as_ref().is_some_and(|id|!s.archive.trials.iter().any(|t|&t.id==id)) { return Err("view.trial_id: unknown trial".into()); }
    if !v.min_drive.is_finite() || !v.max_drive.is_finite() || !(0. ..=1.).contains(&v.min_drive) || !(v.min_drive..=1.).contains(&v.max_drive) { return Err("view.min_drive/max_drive: expected 0 <= minimum <= maximum <= 1".into()); }
    if ![-1,0,1].contains(&v.direction) { return Err("view.direction: expected -1, 0 or 1".into()); }
    if v.device!=0 && !s.archive.trials.iter().any(|t|t.device==v.device) { return Err("view.device: unknown device".into()); }
    if !["All","Tuning","Held out"].contains(&v.role.as_str()) { return Err("view.role: unknown role".into()); }
    if !["All","Pass","Fail","Unscored","Regression","Empirical failure"].contains(&v.outcome.as_str()) { return Err("view.outcome: unknown outcome".into()); }
    Ok(())
}
pub fn filtered_ids(s: &Study) -> Vec<String> {
    s.archive.trials.iter().filter(|t| {
        let r=s.view.evaluation.and_then(|i|s.evaluations.get(i)).and_then(|e|e.results.iter().find(|r|r.trial_id==t.id));
        let pair=r.and_then(|r|r.pair());
        let outcome=r.map(|r|r.outcome()).unwrap_or(super::TrialOutcome::Unscored);
        (s.view.device==0 || s.view.device==t.device) && (s.view.direction==0 || t.drive.signum() as i8==s.view.direction)
        && t.drive.abs()>=s.view.min_drive && t.drive.abs()<=s.view.max_drive
        && (s.view.role=="All" || (s.view.role=="Tuning")== (t.split=="train"))
        && match s.view.outcome.as_str() { "Pass"=>outcome==super::TrialOutcome::Pass, "Fail"=>outcome==super::TrialOutcome::Fail, "Unscored"=>outcome==super::TrialOutcome::Unscored, "Regression"=>pair.is_some_and(|(b,c)|c.metrics.rmse>b.metrics.rmse), "Empirical failure"=>!t.comparison.passes, _=>true }
    }).map(|t|t.id.clone()).collect()
}
pub fn trial_ids(s: &Study, selection: EvaluationSelection) -> Result<Vec<String>,String> {
    validate_view(s)?;
    let ids=match selection { EvaluationSelection::Selected=>s.view.trial_id.clone().into_iter().collect(), EvaluationSelection::Filtered=>filtered_ids(s), EvaluationSelection::HeldOut=>s.archive.trials.iter().filter(|t|t.split!="train" && (s.view.device==0 || t.device==s.view.device)).map(|t|t.id.clone()).collect(), EvaluationSelection::Ids(ids)=>ids };
    if ids.is_empty() { return Err("evaluation.trial_ids: select at least one trial".into()); }
    let mut seen=std::collections::BTreeSet::new();
    for id in &ids { if !seen.insert(id) || !s.archive.trials.iter().any(|t|&t.id==id) { return Err(format!("evaluation.trial_ids.{id}: duplicate or unknown trial")); } }
    Ok(ids)
}
pub fn expose(s: &mut Study, ids: &[String]) {
    if ids.iter().any(|id|s.archive.trials.iter().any(|t|&t.id==id && t.split!="train")) { s.validation_seen=true; }
}
pub fn apply(s: &mut Study, command: Command) -> Result<(),String> {
    let exposes_view=matches!(&command, Command::SetView(_) | Command::SelectEvaluation(_));
    let mut next=s.clone();
    match command {
        Command::SetCandidate(v)=>next.draft=v,
        Command::SetParameter{group,name,value}=>{
            if !metadata().iter().any(|p|p.group==group && p.name==name) { return Err(format!("draft.{group:?}.{name}: parameter is not editable")); }
            match group { ParameterGroup::Motor=>&mut next.draft.motor, ParameterGroup::Bridge=>&mut next.draft.bridge }.insert(name,value);
        }
        Command::SetConditions(v)=>next.draft.conditions=v, Command::SetStep(v)=>next.draft.step_s=v,
        Command::SetLimits(v)=>next.limits=v, Command::SetView(v)=>next.view=v,
        Command::SelectTrial(v)=>next.view.trial_id=v, Command::SelectEvaluation(v)=>next.view.evaluation=v,
        Command::SetNotes(v)=>next.notes=v,
        Command::SetDecision{evaluation,decision,notes}=>{
            if !DECISIONS.contains(&decision.as_str()) { return Err("evaluations.decision: unknown decision".into()); }
            let e=next.evaluations.get_mut(evaluation).ok_or("evaluations: unknown evaluation")?; e.decision=decision;e.notes=notes;
        }
        Command::ResetCandidate=>next.draft=next.baseline.clone(),
        Command::UseEvaluation(i)=>next.draft=next.evaluations.get(i).ok_or("evaluations: unknown evaluation")?.candidate.clone(),
        Command::Expose(selection)=>{ let ids=match selection { EvaluationSelection::Filtered=>filtered_ids(&next), other=>trial_ids(&next,other)? }; expose(&mut next,&ids); }
    }
    next.draft.validate()?; validate_limits(&next.limits)?; validate_view(&next)?;
    // Exposure is monotonic, including before subsequent candidate edits.
    if next.draft!=s.draft || next.limits!=s.limits { next.candidate_edited(); }
    if let Some(id)=next.view.trial_id.clone() { expose(&mut next,&[id]); }
    if let Some(i)=next.view.evaluation { let ids=next.evaluations[i].results.iter().map(|r|r.trial_id.clone()).collect::<Vec<_>>();expose(&mut next,&ids); }
    if exposes_view { let ids=filtered_ids(&next); expose(&mut next,&ids); }
    *s=next; Ok(())
}

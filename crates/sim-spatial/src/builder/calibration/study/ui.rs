//! Existing Build → Actuators → Measured evidence surface. All controls use
//! kit widgets and the same stamped StudyAction path as REST/system_ui.
use super::{StudyOwner, StudyAction, StudyStamp, forms::{StudyUi,Field,Hit}, chart};
use bevy::prelude::*;
use crate::{app::{ViewerSet,InputSet}, builder::ui_api::Enabled, ui_kit::{Kit,UiFonts,Look,Corner,size,TEXT,FAINT,WARN,DANGER,wrap}};
use crate::ui_kit::{text::{TextFieldApp,TextField,EnterKey},path_field::{PathView,PathHit}};
use sim_runtime::experiment_study::commands::{self,Command,EvaluationSelection,ParameterGroup};
use serde_json::{Value,json};
use std::collections::BTreeMap;

/// Captured from actual rendered entities, not an independent control catalogue.
#[derive(Clone,Debug)]
pub(crate) struct RenderedControl { pub(crate) hit:Hit,pub(crate) enabled:bool,pub(crate) label:String }
#[derive(Component)]
pub(crate) struct ControlId(pub(crate) String);
#[derive(Component)]
struct GlobalStatus;

pub(crate) fn register(app:&mut App) {
    app.init_resource::<StudyUi>()
        .add_text_field(super::forms::FIELD,TextField::new("Measured study field").enter(EnterKey::ShiftNewline))
        .add_systems(Update,super::forms::input.in_set(InputSet::Window).in_set(ViewerSet::Input))
        .add_systems(Update,chart::receive.in_set(ViewerSet::JobResults))
        .add_systems(Update,chart::request.in_set(ViewerSet::SimSync))
        .add_systems(Update,(status,collect).chain().in_set(ViewerSet::Present));
}
pub(crate) fn controls(ui:&StudyUi)->Vec<Value> {
    ui.rendered.iter().map(|(id,c)|json!({"id":id,"label":c.label,"enabled":c.enabled,"kind":"button","measured_study":true})).collect()
}
/// Present follows the builder's SimSync panel and sees deferred child spawns.
pub(crate) fn collect(buttons:Query<(Option<&ControlId>,&Hit,Option<&Enabled>,&bevy::ui::prelude::AccessibleLabel),With<Button>>,mut ui:ResMut<StudyUi>) {
    let mut rendered=BTreeMap::new();
    for (id,hit,enabled,label) in &buttons {
        let generated=match hit {
            Hit::Focus{stamp,field,..}=>format!("study:field:{}:{field:?}",stamp.map(|s|s.id).unwrap_or(0)),
            Hit::Path{stamp,field,..}=>format!("study:path:{}:{field:?}",stamp.map(|s|s.id).unwrap_or(0)),
            Hit::Action(_)=>String::new(),
            Hit::Discard{key}=>format!("study:discard:{key:?}"),
            Hit::SelectedComponent{stamp}=>format!("study:link:{}",stamp.id),
        };
        // Action buttons supply an explicit ID; input/path identities derive
        // from their actual stamped component when the kit owns the spawn.
        let id=id.map(|i|i.0.clone()).unwrap_or(generated);
        if !id.is_empty() {rendered.insert(id,RenderedControl{hit:hit.clone(),enabled:enabled.is_none_or(|e|e.0),label:label.0.clone()});}
    }
    let signature=|map:&BTreeMap<String,RenderedControl>|map.iter().map(|(id,c)|(id.clone(),c.enabled,c.label.clone(),format!("{:?}",c.hit))).collect::<Vec<_>>();
    if signature(&ui.rendered)!=signature(&rendered) {ui.rendered=rendered;}
}
fn button(body:&mut ChildSpawnerCommands,k:&Kit,id:impl Into<String>,label:&str,action:StudyAction,enabled:bool) {
    body.spawn(k.button(label,Hit::Action(action),Look::Secondary,enabled)).insert(ControlId(id.into()));
}
fn field(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,field:Field,label:&str,value:String,enabled:bool) {
    let shown=ui.shown(Some(stamp),&field,value);
    body.spawn(k.text(label,size::DETAIL,FAINT,1));
    body.spawn(k.input(&shown,"Enter to apply",Hit::Focus{stamp:Some(stamp),field:field.clone(),text:shown.clone()},ui.focused(Some(stamp),&field)))
        .insert((Enabled(enabled),ControlId(format!("study:field:{}:{field:?}",stamp.id))));
}
fn path(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:Option<StudyStamp>,field:Field,label:&str,submit:&str,enabled:bool) {
    let text=ui.shown(stamp,&field,String::new());
    let view=PathView{label,text:&text,placeholder:label,focused:ui.focused(stamp,&field),selected:false,submit:Some(submit),submit_enabled:enabled,listing:None};
    k.path_field(body,&view,|hit|match hit {
        PathHit::Field=>Hit::Focus{stamp,field:field.clone(),text:text.clone()},
        _=>Hit::Path{stamp,field:field.clone(),text:text.clone()},
    });
    // PathHit buttons carry the same Hit and are collected by their own identity
    // (below in collect); focus inputs use Enabled from kit/custom insertion.
}
pub(crate) fn section(body:&mut ChildSpawnerCommands,k:&Kit,owner:&StudyOwner,ui:&StudyUi) {
    body.spawn(k.section("Offline measured-PWM study"));
    body.spawn(k.caption("Exploratory hypotheses from archived measurements. No hardware acquisition, registry promotion or CAD changes. Power/controller/FPGA refinement remains external."));
    path(body,k,ui,None,Field::Archive,"Identification archive folder","Open archive",!owner.busy());
    path(body,k,ui,None,Field::Review,"Saved study JSON","Open review",!owner.busy());
    if let Some(error)=&ui.error {body.spawn(k.text(error,size::SMALL,DANGER,0));}
    for (key,text) in &ui.drafts {
        body.spawn(k.text(format!("Retained unresolved field {:?}: {}",key,text),size::DETAIL,WARN,0));
        body.spawn(k.button("Discard this unsent field draft",Hit::Discard{key:key.clone()},Look::Secondary,!ui.awaiting.values().any(|(k,_)|k==key)));
    }
    body.spawn(k.text(&owner.status,size::SMALL,TEXT,0));
    for receipt in &owner.receipts {
        body.spawn(k.text(format!("Job {} · {:?} · {}{}{}{}",receipt.id,receipt.kind,receipt.message,if receipt.stale{" · STALE"}else{""},if receipt.displaced{" · DISPLACED"}else{""},receipt.error.as_ref().map(|e|format!(" · {e}")).unwrap_or_default()),size::DETAIL,if receipt.error.is_some(){WARN}else{TEXT},0));
        if let Some(captured)=&receipt.captured {
            body.spawn(k.text(format!("Captured revision {:?} · trial IDs {} · baseline {:?} · candidate {:?} · limits {:?}",receipt.stamp,receipt.trial_ids.join(", "),captured.baseline,captured.draft,captured.limits),size::DETAIL,FAINT,0));
        }
    }
    body.spawn(wrap()).with_children(|row|for retained in &owner.studies {
        button(row,k,format!("study:choose:{}",retained.id),&format!("Study {}{}",retained.id,if retained.dirty(){" · unsaved"}else{""}),StudyAction::Choose{id:retained.id},true);
    });
    let Some(retained)=owner.active() else {body.spawn(k.caption("Open an archive or a saved review to author a candidate."));return};
    let stamp=retained.stamp(); let s=&retained.study;
    let usable=retained.displaced.is_none();
    body.spawn(k.text(format!("Study {} · revision {} · {} · source {}",retained.id,retained.revision,if retained.dirty(){"UNSAVED"}else{"saved"},retained.source),size::DETAIL,TEXT,0));
    body.spawn(k.text(format!("Archive {} · verified raw input hashes {} / {}",s.archive.label,s.archive.verified_inputs,s.archive.input_blake3.len()),size::DETAIL,TEXT,0));
    if let Some(reason)=&retained.displaced {body.spawn(k.text(format!("DISPLACED retained evidence: {reason}"),size::SMALL,WARN,1));}
    body.spawn(k.text(format!("Source observations {} · archive model {}",s.archive.observation_blake3,s.archive.model_blake3),size::DETAIL,FAINT,0));
    body.spawn(k.caption(&s.archive.interpretation));body.spawn(k.caption(&s.archive.split_policy));
    for issue in &s.archive.integrity_issues {body.spawn(k.text(issue,size::DETAIL,WARN,0));}
    body.spawn(k.text(format!("Validation viewed: {} · candidate influenced: {}",s.validation_seen,s.validation_influenced),size::DETAIL,if s.validation_influenced{WARN}else{TEXT},1));
    body.spawn(k.text(format!("Related system component: {}",s.view.component_id.as_deref().unwrap_or("none")),size::DETAIL,TEXT,0));
    body.spawn(k.button("Link the shared selected Build component",Hit::SelectedComponent{stamp},Look::Secondary,usable));
    let mut cleared=s.view.clone();cleared.component_id=None;
    button(body,k,"study:clear_link","Clear component reference",StudyAction::Apply{stamp,command:Command::SetView(cleared)},usable);
    for (key,value) in &s.retained_fields {
        if key.starts_with("native_offline_job_receipts") {
            if let Some(rows)=value.as_array(){for row in rows {
                body.spawn(k.text(format!("Saved job receipt: kind {} · {} · error {} · captured inputs {}",row.get("kind").unwrap_or(&Value::Null),row.get("message").unwrap_or(&Value::Null),row.get("error").unwrap_or(&Value::Null),row.get("inputs").unwrap_or(&Value::Null)),size::DETAIL,WARN,0));
            }}
        }
    }
    candidate(body,k,ui,stamp,s,usable);
    for (name,label,value) in [("device","Device filter · 0: all",s.view.device.to_string()),("direction","Direction filter · -1 / 0 (all) / 1",s.view.direction.to_string()),("min_drive","Minimum |PWM| fraction [0…1]",s.view.min_drive.to_string()),("max_drive","Maximum |PWM| fraction [0…1]",s.view.max_drive.to_string())] {
        field(body,k,ui,stamp,Field::Filter(name),label,value,usable);
    }
    trials(body,k,stamp,s,usable);
    body.spawn(wrap()).with_children(|row| {
        for (id,label,set) in [("selected","Evaluate selected",EvaluationSelection::Selected),("filtered","Evaluate filtered",EvaluationSelection::Filtered),("heldout","Evaluate held-out",EvaluationSelection::HeldOut)] {
            let enabled=usable && !owner.busy() && commands::trial_ids(s,set.clone()).is_ok();
            button(row,k,format!("study:evaluate:{id}"),label,StudyAction::Evaluate{stamp,set},enabled);
        }
    });
    body.spawn(k.caption("Evaluation captures baseline and candidate, exact trial IDs, limits, conditions, split exposure and runtime identity before its job starts. Edits during evaluation make old evidence stale."));
    comparison(body,k,owner,ui,stamp,s,usable);
    field(body,k,ui,stamp,Field::Notes,"Study notes",s.notes.clone(),usable);
    path(body,k,ui,Some(stamp),Field::Save,"New review JSON destination","Save new",!owner.busy());
    path(body,k,ui,Some(stamp),Field::Export,"New HTML destination","Export HTML new",!owner.busy());
    body.spawn(k.caption("Save/export publish a captured revision and refuse existing destinations. Later edits remain unsaved. Reopen a saved review using Open review above."));
}
fn candidate(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,s:&sim_runtime::experiment_study::Study,usable:bool) {
    body.spawn(k.section("Candidate parameters · registry metadata"));
    for p in commands::metadata() {
        let bridge=p.group==ParameterGroup::Bridge;
        let map=if bridge{&s.draft.bridge}else{&s.draft.motor};
        let value=map.get(&p.name).copied().or(p.default).map(|v|v.to_string()).unwrap_or_default();
        let label=format!("{}.{} [{}] · {}{}…{}{}",if bridge{"bridge"}else{"motor"},p.name,p.unit,if p.exclusive_minimum{" > "}else{" ≥ "},p.minimum.map(|v|v.to_string()).unwrap_or("−∞".into()),p.maximum.map(|v|v.to_string()).unwrap_or("∞".into()),if p.integer{" · integer"}else{""});
        field(body,k,ui,stamp,Field::Parameter(bridge,p.name),&label,value,usable);
    }
    body.spawn(k.section("Conditions and timestep"));
    let c=&s.draft.conditions;
    for (name,label,value) in [
        ("voltage_v","Voltage [V] · blank: each trial's reported midpoint",c.voltage_v.map(|v|v.to_string()).unwrap_or_default()),
        ("temperature_c","Temperature [°C] · blank: trial midpoint",c.temperature_c.map(|v|v.to_string()).unwrap_or_default()),
        ("load_inertia","Load inertia [kg m²]",c.load_inertia.to_string()),
        ("load_torque","Load torque [N m]",c.load_torque.to_string()),
        ("command_delay_s","Command delay [s]",c.command_delay_s.to_string()),
    ] {field(body,k,ui,stamp,Field::Condition(name),label,value,usable);}
    field(body,k,ui,stamp,Field::Step,"Simulation step [s] · 10 µs–2 ms",s.draft.step_s.to_string(),usable);
    if s.draft.power.is_some() {body.spawn(k.text("Retained explicit power payload: broader power authoring remains external; fixed voltage overrides may be refused by shared validation.",size::DETAIL,WARN,0));}
    body.spawn(k.section("Comparison limits"));
    let limits=s.limits.as_ref().or_else(||s.archive.trials.first().map(|t|&t.limits));
    if let Some(l)=limits {
        field(body,k,ui,stamp,Field::Limit("rmse"),"RMSE limit [rad]",l.rmse.to_string(),usable);
        field(body,k,ui,stamp,Field::Limit("final_abs_error"),"Final absolute error limit [rad]",l.final_abs_error.to_string(),usable);
    }
    button(body,k,"study:archive_limits","Use each archived trial's limits",StudyAction::Apply{stamp,command:Command::SetLimits(None)},usable);
    button(body,k,"study:reset_candidate","Reset candidate to baseline",StudyAction::Apply{stamp,command:Command::ResetCandidate},usable);
    body.spawn(k.text(format!("Current baseline → candidate changes: {}",sim_runtime::experiment_study::changes(&s.baseline,&s.draft).join("; ")),size::DETAIL,TEXT,0));
}
fn trials(body:&mut ChildSpawnerCommands,k:&Kit,stamp:StudyStamp,s:&sim_runtime::experiment_study::Study,usable:bool) {
    body.spawn(k.section("Trial selection and filters"));
    // All filter and exposure behavior is the shared SetView command.
    for (name,options) in [("role",vec!["All","Tuning","Held out"]),("outcome",vec!["All","Pass","Fail","Unscored","Regression","Empirical failure"])] {
        body.spawn(wrap()).with_children(|row|for option in options {
            let mut view=s.view.clone(); if name=="role"{view.role=option.into()}else{view.outcome=option.into()}
            button(row,k,format!("study:filter:{name}:{option}"),option,StudyAction::Apply{stamp,command:Command::SetView(view)},usable);
        });
    }
    // Numeric filters are drawn in section via the same field helper.
    let visible=commands::filtered_ids(s);
    body.spawn(k.text(format!("{} matching trials · outcome uses complete captured pairs; empirical failure uses archive comparison",visible.len()),size::DETAIL,FAINT,0));
    for id in visible {
        if let Some(t)=s.archive.trials.iter().find(|t|t.id==id) {
            let label=format!("{}{} · {} · device {} · {:+.1}% · archive {}",if s.view.trial_id.as_ref()==Some(&id){"▶ "}else{""},id,t.split,t.device,t.drive*100.,if t.comparison.passes{"pass"}else{"fail"});
            button(body,k,format!("study:trial:{id}"),&label,StudyAction::Apply{stamp,command:Command::SelectTrial(Some(id))},usable);
        }
    }
}
fn comparison(body:&mut ChildSpawnerCommands,k:&Kit,owner:&StudyOwner,ui:&StudyUi,stamp:StudyStamp,s:&sim_runtime::experiment_study::Study,usable:bool) {
    body.spawn(k.section("Captured comparisons"));
    button(body,k,"study:evaluation:archive","Archive predictions only",StudyAction::Apply{stamp,command:Command::SelectEvaluation(None)},usable);
    for (i,e) in s.evaluations.iter().enumerate() {
        button(body,k,format!("study:evaluation:{i}"),&format!("{}{} · {}",e.id,if e.cancelled{" · cancelled"}else{""},e.decision),StudyAction::Apply{stamp,command:Command::SelectEvaluation(Some(i))},usable);
    }
    let evaluation=chart::evaluation(s);
    if let Some(e)=evaluation {
        let stale=e.stale(&s.baseline,&s.draft,&s.limits);
        body.spawn(k.text(format!("{} · {}{} · {}",e.id,if stale{"STALE: draft settings or limits changed"}else{"captured settings match draft"},if e.cancelled{" · cancelled; incomplete trials unscored"}else{""},e.decision),size::SMALL,if stale||e.cancelled{WARN}else{TEXT},1));
        body.spawn(k.caption(&e.assumptions));
        body.spawn(k.text(format!("Runtime {} · features {:?} · integrator {:?} · seed {} · validation influenced {}",e.runtime.library_source_blake3,e.runtime.features,e.integrator,e.seed,e.validation_influenced),size::DETAIL,FAINT,0));
        if e.runtime!=sim_runtime::physics_context::RuntimeIdentity::current(){body.spawn(k.text("STALE runtime identity: this evaluation was captured with different library source/features.",size::DETAIL,WARN,1));}
        body.spawn(k.text(format!("Captured source/model/document identity and split exposure: {:?}",e.capture),size::DETAIL,FAINT,0));
        body.spawn(k.text(format!("Captured baseline {} · candidate {} · limits {:?}",e.baseline.fingerprint(),e.candidate.fingerprint(),e.limits),size::DETAIL,FAINT,0));
        body.spawn(k.text(format!("Captured changes: {}",sim_runtime::experiment_study::changes(&e.baseline,&e.candidate).join("; ")),size::DETAIL,TEXT,0));
        body.spawn(k.text(format!("Captured baseline settings: {:?}",e.baseline),size::DETAIL,FAINT,0));
        body.spawn(k.text(format!("Captured candidate settings: {:?}",e.candidate),size::DETAIL,FAINT,0));
        body.spawn(k.text(format!("Captured trial IDs: {}",e.results.iter().map(|r|r.trial_id.as_str()).collect::<Vec<_>>().join(", ")),size::DETAIL,FAINT,0));
        let summary=e.summary(&commands::filtered_ids(s));
        body.spawn(k.text(format!("Filtered captured summary: {} pass · {} fail · {} unscored / {} · {} improved · {} regressed · {} new failures",summary.passes,summary.failures,summary.unscored,summary.total,summary.improved,summary.regressed,summary.new_failures),size::DETAIL,TEXT,0));
        if let Some(i)=s.view.evaluation {
            body.spawn(wrap()).with_children(|row|for decision in commands::DECISIONS {
                button(row,k,format!("study:decision:{i}:{decision}"),decision,StudyAction::Apply{stamp,command:Command::SetDecision{evaluation:i,decision:(*decision).into(),notes:e.notes.clone()}},usable);
            });
            field(body,k,ui,stamp,Field::DecisionNotes(i),"Evaluation notes",e.notes.clone(),usable);
            button(body,k,format!("study:use:{i}"),"Copy captured candidate to draft",StudyAction::Apply{stamp,command:Command::UseEvaluation(i)},usable);
        }
    }
    let Some(t)=s.view.trial_id.as_ref().and_then(|id|s.archive.trials.iter().find(|t|&t.id==id)) else {return};
    body.spawn(k.text(format!("Trial {} · {} · {} [{}]",t.id,t.split,t.measured.quantity.name,t.measured.unit),size::SMALL,TEXT,1));
    body.spawn(k.text(format!("Run {} · device {} · stage {} · kind {} · drive {:+.1}% · duration {} s · voltage {:?} V · temperature {:?} °C",t.run,t.device,t.stage,t.kind,t.drive*100.,t.duration_s,t.voltage_range_v,t.temperature_range_c),size::DETAIL,FAINT,0));
    body.spawn(k.text(format!("Captured command on {:?} s · off {:?} s · driver release {:?}",t.on_host_window_s,t.off_host_window_s,t.release),size::DETAIL,FAINT,0));
    body.spawn(k.text(format!("Archive comparison: RMSE {} · final {} · max {} · {}",t.comparison.rmse,t.comparison.final_error,t.comparison.maximum_abs_error,if t.comparison.passes{"pass"}else{"fail"}),size::DETAIL,TEXT,0));
    let pair=evaluation.and_then(|e|e.results.iter().find(|r|r.trial_id==t.id));
    let samples=[t.measured.samples.len(),t.predicted.samples.len(),pair.and_then(|r|r.baseline.as_ref()).map(|p|p.trace.samples.len()).unwrap_or(0),pair.and_then(|r|r.candidate.as_ref()).map(|p|p.trace.samples.len()).unwrap_or(0)];
    for (i,label) in chart::LABELS.iter().enumerate() {
        let [r,g,b]=crate::chart::COLORS[i]; body.spawn(k.text(format!("{label} · {}",if samples[i]==0{"unavailable; no line".into()}else{format!("{} samples",samples[i])}),size::DETAIL,Color::srgb_u8(r,g,b),1));
    }
    if let Some(result)=evaluation.and_then(|e|e.results.iter().find(|r|r.trial_id==t.id)) {
        let scored=result.errors.is_empty() && result.baseline.is_some() && result.candidate.is_some();
        if !scored {body.spawn(k.text("UNSCORED: failed, cancelled or incomplete baseline/candidate pair",size::DETAIL,WARN,1));}
        for error in &result.errors {body.spawn(k.text(error,size::DETAIL,DANGER,0));}
        for (label,prediction) in [("Captured baseline",&result.baseline),("Captured candidate",&result.candidate)] {
            if let Some(p)=prediction {body.spawn(k.text(format!("{label}: RMSE {} · final {} · max {}{}",p.metrics.rmse,p.metrics.final_error,p.metrics.maximum_abs_error,if scored{if p.metrics.passes{" · pass"}else{" · fail"}}else{" · unscored"}),size::DETAIL,TEXT,0));}
            else {body.spawn(k.text(format!("{label}: unavailable · unscored"),size::DETAIL,WARN,0));}
        }
    }
    if ui.chart.current(owner) {
        if let Some(image)=&ui.chart.image {
            body.spawn(k.chart_image(image.clone(),Node{width:Val::Percent(100.),height:Val::Px(180.),..default()},true)).with_children(|plot|{
                plot.spawn(k.chart_label(format!("{} {}",ui.chart.axes.0.1,t.measured.unit),Corner::TopLeft));
                plot.spawn(k.chart_label(format!("{} {}",ui.chart.axes.0.0,t.measured.unit),Corner::BottomLeft));
                plot.spawn(k.chart_label(format!("{}–{} s",ui.chart.axes.1.0,ui.chart.axes.1.1),Corner::BottomRight));
            });
        }
    } else {body.spawn(k.caption("Preparing captured trace chart…"));}
    if let Some(error)=&ui.chart.error {body.spawn(k.text(error,size::DETAIL,DANGER,0));}
}
/// Global cancellation/progress remains visible when the dock closes or mode
/// changes. This root is transient presentation, never owns evidence or jobs.
pub(crate) fn status(mut commands:Commands,old:Query<Entity,With<GlobalStatus>>,fonts:Option<Res<UiFonts>>,owner:Res<StudyOwner>,ui:Res<StudyUi>,mut drawn:Local<Option<String>>) {
    let Some(fonts)=fonts else {return};
    let key=format!("{}|{}|{:?}|{:?}|{:?}|{:?}",owner.changed,owner.status,ui.blocking_reason(),ui.focus,ui.error,owner.pending.iter().map(|p|(p.id,p.cancel_requested,p.progress())).collect::<Vec<_>>());
    if drawn.as_ref()==Some(&key) {return}
    *drawn=Some(key);
    for entity in &old {commands.entity(entity).despawn();}
    if owner.pending.is_empty() && owner.blocking_reason().is_none() && owner.status.is_empty() && ui.blocking_reason().is_none() && ui.error.is_none() {return}
    let k=Kit::new(&fonts);
    commands.spawn((GlobalStatus,crate::app::Persistent,Node{position_type:PositionType::Absolute,right:Val::Px(12.),top:Val::Px(12.),max_width:Val::Px(460.),flex_direction:FlexDirection::Column,..default()})).with_children(|body|{
        body.spawn(k.caption("Retained measured study work"));
        if !owner.status.is_empty(){body.spawn(k.text(&owner.status,size::DETAIL,TEXT,0));}
        if ui.focus.is_some(){body.spawn(k.text("Measured field draft retained. Return to Measured evidence to submit, or press Escape to release focus.",size::DETAIL,WARN,0));}
        if let Some(reason)=ui.blocking_reason(){body.spawn(k.text(reason,size::DETAIL,WARN,0));}
        if let Some(error)=&ui.error{body.spawn(k.text(error,size::DETAIL,DANGER,0));}
        if let Some(reason)=owner.blocking_reason(){body.spawn(k.text(reason,size::DETAIL,WARN,0));}
        for job in &owner.pending {
            let p=job.progress();
            body.spawn(k.text(format!("Job {} · {:?} · {:?} · {}",job.id,job.kind,p.steps,p.message),size::DETAIL,TEXT,0));
            button(body,&k,format!("study:cancel:{}",job.id),if job.cancel_requested{"Cancellation requested"}else{"Cancel measured study job"},StudyAction::Cancel{job:job.id},!job.cancel_requested);
        }
        for receipt in owner.receipts.iter().rev().take(3) {body.spawn(k.text(format!("Job {} · {}{}{}{}",receipt.id,receipt.message,if receipt.stale{" · STALE"}else{""},if receipt.displaced{" · DISPLACED"}else{""},receipt.error.as_ref().map(|e|format!(" · {e}")).unwrap_or_default()),size::DETAIL,TEXT,0));}
    });
}
#[cfg(test)]
#[path="ui_tests.rs"]
mod tests;

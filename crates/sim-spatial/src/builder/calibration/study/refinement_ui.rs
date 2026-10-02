//! Kit authoring rows and captured refinement evidence. No execution or truth ownership.
use super::{button,field};
use super::super::{StudyOwner,StudyAction,StudyStamp,forms::{StudyUi,Field,Hit}};
use bevy::prelude::*;
use crate::ui_kit::{Kit,Look,size,TEXT,FAINT,WARN,wrap};
use sim_runtime::{experiment_study::{Study,refinement::{Command,Operation}},controller_refinement::{control::{Policy,Knot},calibration::{Coordinate,Variant}}};
use serde_json::Value;

fn apply(body:&mut ChildSpawnerCommands,k:&Kit,stamp:StudyStamp,id:&str,label:&str,command:Command,enabled:bool) {
    button(body,k,format!("study:refine:{id}"),label,StudyAction::RefineApply{stamp,command},enabled);
}
/// Enumerate typed scalar leaves so every timing, policy, task and row field has
/// its own actual kit input. Excluded electrical payloads remain opaque.
fn leaves(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,group:&str,pointer:&str,value:&Value,enabled:bool) {
    match value {
        Value::Object(map)=>for (name,v) in map {
            let schema_field=group=="experiment" && ((pointer.is_empty() && (name=="version"||name=="electrical")) || (pointer=="/controller" && (name=="kind" || (name=="parameters" && map.get("kind").and_then(Value::as_str)==Some("rhai")))));
            let scenario_power=group=="scenarios" && pointer.ends_with("/model") && name=="power";
            if schema_field||scenario_power {continue}
            let escaped=name.replace('~',"~0").replace('/',"~1");
            leaves(body,k,ui,stamp,group,&format!("{pointer}/{escaped}"),v,enabled);
        },
        Value::Array(rows)=>for (i,v) in rows.iter().enumerate(){leaves(body,k,ui,stamp,group,&format!("{pointer}/{i}"),v,enabled)},
        _=>{
            let text=match value {Value::String(s)=>s.clone(),Value::Null=>String::new(),_=>value.to_string()};
            let label=format!("{group}{}{}",pointer.replace('/'," · "),if pointer.ends_with("/device")&&group=="coordinates"{" · blank: shared; otherwise device delta"}else{""});
            field(body,k,ui,stamp,Field::Refinement(format!("{group}:{pointer}")),&label,text,enabled);
        }
    }
}
pub(crate) fn section(body:&mut ChildSpawnerCommands,k:&Kit,owner:&StudyOwner,ui:&StudyUi,stamp:StudyStamp,s:&Study,usable:bool) {
    let w=&s.refinement;let enabled=usable;
    body.spawn(k.section("Offline controller refinement"));
    body.spawn(k.caption("Exploratory controller and physical-model drafts. Timing evidence and task limits are provisional until measured and qualified. Electrical and power payloads remain retained externally."));
    body.spawn(wrap()).with_children(|row|{
        for (id,policy) in [("pid",Policy::default()),("rhai",Policy::Rhai{source:"fn control(t, sensors, actuators, state) { #{commands: #{duty: 0.0}, state: state} }".into(),parameters:serde_json::json!({"gain":1.0}),duty_limit:0.5})] {
            let mut e=w.experiment.clone();e.controller=policy;
            apply(row,k,stamp,id,if id=="pid"{"Use Rust PID draft"}else{"Use Rhai draft"},Command::SetExperiment(e),enabled);
        }
    });
    if let Policy::Rhai{parameters,..}=&w.experiment.controller {
        field(body,k,ui,stamp,Field::Refinement("parameter_new:".into()),"Add Rhai parameter row · name = JSON value",String::new(),enabled);
        if let Some(rows)=parameters.as_object() {for (name,value) in rows {
            field(body,k,ui,stamp,Field::Refinement(format!("parameter_value:{name}")),&format!("Rhai parameter {name} · JSON value"),value.to_string(),enabled);
            let mut e=w.experiment.clone();
            if let Policy::Rhai{parameters,..}=&mut e.controller {if let Some(map)=parameters.as_object_mut(){map.remove(name);}}
            apply(body,k,stamp,&format!("parameter:remove:{name}"),&format!("Remove Rhai parameter {name}"),Command::SetExperiment(e),enabled);
        }}
    }
    if let Ok(v)=serde_json::to_value(&w.experiment){leaves(body,k,ui,stamp,"experiment","",&v,enabled)}
    let mut e=w.experiment.clone();
    if let Some(last)=e.trajectory.last().cloned(){
        let next=last.time_s+(e.duration_s-last.time_s)/2.;
        e.trajectory.push(Knot{time_s:next,position_rad:last.position_rad});
        apply(body,k,stamp,"knot:add","Add trajectory knot before duration",Command::SetExperiment(e),enabled&&next>last.time_s);
    }
    for i in 0..w.experiment.trajectory.len(){let mut e=w.experiment.clone();e.trajectory.remove(i);apply(body,k,stamp,&format!("knot:remove:{i}"),&format!("Remove trajectory row {i}"),Command::SetExperiment(e),enabled&&i>0&&w.experiment.trajectory.len()>2);}
    body.spawn(k.section("Bounded coordinates · archive analysis"));
    if let Ok(v)=serde_json::to_value(&w.coordinates){leaves(body,k,ui,stamp,"coordinates","",&v,enabled)}
    for i in 0..w.coordinates.len(){let mut c=w.coordinates.clone();c.remove(i);apply(body,k,stamp,&format!("coordinate:remove:{i}"),&format!("Remove coordinate {i}"),Command::SetCoordinates(c),enabled);}
    let mut c=w.coordinates.clone();
    let available=[("condition.load_torque",s.draft.conditions.load_torque),("condition.load_inertia",s.draft.conditions.load_inertia),("condition.delay",s.draft.conditions.command_delay_s)].into_iter().find(|(path,_)|!c.iter().any(|v|v.path==*path&&v.device.is_none()));
    if let Some((path,value))=available{c.push(Coordinate{path:path.into(),device:None,lower:if path=="condition.load_torque"{value-value.abs()-1.}else{0.},upper:value+value.abs()+1.});}
    apply(body,k,stamp,"coordinate:add","Add coordinate row · edit path/device/bounds",Command::SetCoordinates(c),enabled&&available.is_some());
    body.spawn(k.section("Robustness scenarios"));
    if let Ok(v)=serde_json::to_value(&w.scenarios){leaves(body,k,ui,stamp,"scenarios","",&v,enabled)}
    for i in 0..w.scenarios.len(){let mut v=w.scenarios.clone();v.remove(i);apply(body,k,stamp,&format!("scenario:remove:{i}"),&format!("Remove scenario {i}"),Command::SetScenarios(v),enabled);}
    let mut v=w.scenarios.clone();let mut number=v.len()+1;
    while v.iter().any(|v|v.label==format!("scenario {number}")){number+=1;}
    v.push(Variant{label:format!("scenario {number}"),model:s.draft.clone(),timing:w.experiment.timing.clone(),evidence:"Exploratory scenario; edit assumptions before interpretation".into()});
    apply(body,k,stamp,"scenario:add","Add scenario from current model/timing",Command::SetScenarios(v),enabled);
    let default_train=s.archive.trials.iter().filter(|t|t.split=="train").map(|t|t.id.clone()).collect::<Vec<_>>();
    let default_validation=s.archive.trials.iter().filter(|t|t.split!="train").map(|t|t.id.clone()).collect::<Vec<_>>();
    let default_selected=s.view.trial_id.iter().cloned().collect::<Vec<_>>();
    for (role,defaults) in [("train",&default_train),("validation",&default_validation),("sensitivity",&default_selected)]{
        let selected=s.refinement_evidence.selections.get(role).unwrap_or(defaults);
        for t in &s.archive.trials{
            if role=="train"&&t.split!="train"||role=="validation"&&t.split=="train"{continue}
            body.spawn(k.button(format!("{} {role}: {} · frozen {}",if selected.contains(&t.id){"✓"}else{"○"},t.id,t.split),Hit::RefineTrial{stamp,role:role.into(),id:t.id.clone(),defaults:defaults.clone()},Look::Secondary,enabled));
        }
    }
    let train=s.refinement_evidence.selections.get("train").cloned().unwrap_or(default_train);
    let validation=s.refinement_evidence.selections.get("validation").cloned().unwrap_or(default_validation);
    let selected=s.refinement_evidence.selections.get("sensitivity").cloned().unwrap_or(default_selected);
    body.spawn(k.caption(format!("Sensitivity selected IDs {selected:?}; fit frozen archive tuning IDs {train:?}; held-out IDs {validation:?}. Selecting held-out evidence records exposure; fitting never automatically adopts a candidate.")));
    for (id,label,operation) in [("simulate","Simulate controller",Operation::Simulate),("sensitivity","Analyze selected trial sensitivity",Operation::Sensitivity{selected}),("fit","Fit frozen archive splits",Operation::Fit{train,validation}),("robustness","Run robustness scenarios",Operation::Robustness)]{
        button(body,k,format!("study:refine:run:{id}"),label,StudyAction::RefineRun{stamp,operation},usable&&!owner.busy());
    }
    if ui.refinement_charts.current(owner){
        for (label,image,axes) in [("Selected captured target / feedback / physical angle [rad]",&ui.refinement_charts.angle,ui.refinement_charts.angle_axes),("Selected captured applied duty [fraction]",&ui.refinement_charts.duty,ui.refinement_charts.duty_axes)]{
            body.spawn(k.caption(format!("{label} · actual time {}–{} s · range {}–{}",axes.1.0,axes.1.1,axes.0.0,axes.0.1)));
            if let Some(image)=image{body.spawn(k.chart_image(image.clone(),Node{width:Val::Percent(100.),height:Val::Px(180.),..default()},true));}
        }
    }
    if let Some(error)=&ui.refinement_charts.error{body.spawn(k.text(error,size::DETAIL,WARN,0));}
    review(body,k,ui,stamp,s,usable);
}
fn comparison(body:&mut ChildSpawnerCommands,k:&Kit,matches:bool) {
    body.spawn(k.caption(if matches {"Captured analysis inputs match current draft; captured runtime qualification remains unverified"} else {"STALE to current draft: captured inputs differ; captured runtime qualification remains unverified"}));
}
fn selected(s:&Study,kind:&str)->Vec<String> {
    s.refinement_evidence.selections.get(kind).cloned().unwrap_or_else(||match kind {
        "train"=>s.archive.trials.iter().filter(|t|t.split=="train").map(|t|t.id.clone()).collect(),
        "validation"=>s.archive.trials.iter().filter(|t|t.split!="train").map(|t|t.id.clone()).collect(),
        _=>s.view.trial_id.iter().cloned().collect(),
    })
}
fn review(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,s:&Study,enabled:bool){
    let w=&s.refinement;body.spawn(k.section("Captured refinement review"));
    for (i,run) in w.controller_runs.iter().enumerate(){
        apply(body,k,stamp,&format!("chart-run:{i}"),&format!("Inspect full captured traces for run {i}"),Command::SelectControllerRun(i),enabled);
        body.spawn(k.text(format!("Controller run {i} · {} · runtime {} · {}",run.evidence_kind,run.runtime.library_source_blake3,if run.experiment==w.experiment&&run.model==s.draft{"captured inputs match draft"}else{"STALE to current draft"}),size::DETAIL,TEXT,1));
        body.spawn(k.text(format!("Captured experiment {:?} · model {:?}",run.experiment,run.model),size::DETAIL,FAINT,0));
        let score=if run.cancelled||run.failure.is_some(){None}else{run.score.as_ref()};
        body.spawn(k.text(format!("{} · cancellation executed {} · failure {:?}",score.map(|v|format!("RMS {} rad · peak {} rad · settled {} rad · saturation {} · {}",v.rms_rad,v.peak_rad,v.settled_error_rad,v.saturation_fraction,if v.passes{"pass against captured task limits"}else{"fail"})).unwrap_or("UNSCORED incomplete/failed/cancelled".into()),run.cancelled,run.failure),size::DETAIL,if score.is_some(){TEXT}else{WARN},0));
        body.spawn(k.caption(format!("{} captured controller frames and {} physical samples; first eight shown; full selected-run traces charted and full evidence retained for publication.",run.frames.len(),run.truth.len())));
        body.spawn(k.caption("Actual sample-time traces: target/feedback/applied duty use captured frame time; physical angle uses independent captured truth time. Duty is dimensionless."));
        for f in run.frames.iter().take(8){body.spawn(k.mono(format!("t={:.6} s · target={:.6} rad · feedback={:.6} rad · duty={:.6}",f.time_s,f.target_rad,f.estimated_position_rad,f.applied_duty),size::DETAIL,FAINT));}
        for p in run.truth.iter().take(8){body.spawn(k.mono(format!("physical t={:.6} s · angle={:.6} rad",p[0],p[1]),size::DETAIL,FAINT));}
        decision(body,k,ui,stamp,s,"controller",i,enabled);
    }
    for (i,a) in w.sensitivities.iter().enumerate(){comparison(body,k,a.source_model.shared==s.draft && a.coordinates==w.coordinates && a.trial_ids==selected(s,"sensitivity"));body.spawn(k.text(format!("Sensitivity {i} · rank {} · norms {:?} · singular {:?} · correlations {:?} · warnings {:?} · trials {:?} · coordinates {:?} · runtime {}",a.rank,a.column_norms,a.singular_values,a.similar_parameters,a.warnings,a.trial_ids,a.coordinates,a.runtime.library_source_blake3),size::DETAIL,TEXT,0));decision(body,k,ui,stamp,s,"sensitivity",i,enabled);}
    for (i,f) in w.fits.iter().enumerate(){
        comparison(body,k,f.baseline.shared==s.draft && f.coordinates==w.coordinates && f.training_ids==selected(s,"train") && f.validation_ids==selected(s,"validation"));
        let selected_ids=f.training_ids.iter().chain(&f.validation_ids).collect::<std::collections::BTreeSet<_>>();
        let scored_ids=f.scores.iter().map(|v|&v.id).collect::<std::collections::BTreeSet<_>>();
        let complete=!selected_ids.is_empty() && f.scores.len()==selected_ids.len() && selected_ids==scored_ids && f.has_verified_traces() && f.scores.iter().all(|v|v.failure.is_none() && v.baseline.is_some() && v.candidate.is_some());
        if !complete {body.spawn(k.text("UNSCORED legacy or incomplete fit: captured prediction traces and complete comparisons are required for candidate use",size::DETAIL,WARN,0));}
        body.spawn(k.text(format!("Fit {i} · {} · tuning {:?} · validation {:?} · influence {} · runtime {} · optimizer {:?}",f.status,f.training_ids,f.validation_ids,f.validation_influenced,f.runtime.library_source_blake3,f.optimizer),size::DETAIL,TEXT,0));
        for score in &f.scores{body.spawn(k.text(format!("Trial {} device {} split {} · baseline {:?} · candidate {:?} · failure {:?}",score.id,score.device,score.split,score.baseline,score.candidate,score.failure),size::DETAIL,if score.failure.is_some(){WARN}else{TEXT},0));}
        apply(body,k,stamp,&format!("use-fit:{i}"),"Explicitly use fit candidate for this experiment device",Command::UseFit{fit:i,device:Some(w.experiment.device)},enabled&&complete);
        decision(body,k,ui,stamp,s,"fit",i,enabled);
    }
    for (i,a) in w.fit_attempts.iter().enumerate(){
        if let Some(partial)=&a.partial {
            body.spawn(k.text(format!("UNSCORED partial fit attempt {i} · optimizer {:?}",partial.optimizer),size::DETAIL,WARN,0));
            for score in &partial.scores {body.spawn(k.text(format!("Partial trial {} · frozen {} · baseline {:?} · candidate {:?} · failure {:?}",score.id,score.split,score.baseline,score.candidate,score.failure),size::DETAIL,WARN,0));}
        }
        comparison(body,k,a.request.model.shared==s.draft && a.request.coordinates==w.coordinates && a.request.training_ids==selected(s,"train") && a.request.validation_ids==selected(s,"validation"));body.spawn(k.text(format!("Fit attempt {i} · request {:?} · failure {:?} · cancelled {} · {}",a.request,a.failure,a.cancelled,if a.outcome.is_none(){"UNSCORED; no complete candidate"}else{"captured outcome retained; no automatic adoption"}),size::DETAIL,WARN,0));decision(body,k,ui,stamp,s,"fit_attempt",i,enabled);}
    for (i,r) in w.robustness.iter().enumerate(){let current=if w.scenarios.is_empty(){sim_runtime::experiment_study::refinement::default_scenarios(s)}else{w.scenarios.clone()};comparison(body,k,r.experiment==w.experiment && r.variants==current);body.spawn(k.text(format!("Robustness {i} · {} · cancelled {} · failures {:?} · captured variants {:?}",r.interpretation,r.cancelled,r.failures,r.variants),size::DETAIL,TEXT,0));for (label,source,run) in &r.runs{let score=if r.cancelled||!r.failures.is_empty()||r.runs.len()!=r.variants.len()||run.cancelled||run.failure.is_some(){None}else{run.score.as_ref()};body.spawn(k.text(format!("Scenario {label} · source {source} · score {:?} · {}",score,if score.is_none(){"UNSCORED"}else{"captured task limits"}),size::DETAIL,TEXT,0));}decision(body,k,ui,stamp,s,"robustness",i,enabled);}
    for d in &s.refinement_evidence.decisions {body.spawn(k.text(format!("Review {} {} · {} · {}",d.kind,d.index,d.decision,d.notes),size::DETAIL,TEXT,0));}
    for c in &s.refinement_evidence.candidate_uses {body.spawn(k.text(format!("Explicit candidate source fit {} hash {} device {:?} · validation seen {} influenced {} · draft {}",c.fit,c.fit_blake3,c.device,c.validation_seen,c.validation_influenced,c.draft_blake3),size::DETAIL,WARN,0));}
    for r in &s.refinement_evidence.receipts {body.spawn(k.text(format!("Captured refinement receipt {:?} · runtime {} · inputs {} · cancelled {} · failure {:?}",r.operation,r.runtime,r.inputs,r.cancelled,r.failure),size::DETAIL,FAINT,0));}
    for (name,raw) in s.retained_fields.iter().filter(|(name,_)|name.starts_with("native_form_inputs")) {body.spawn(k.text(format!("Retained form evidence {name} (raw intent, not applied settings): {raw}"),size::DETAIL,WARN,0));}
    for f in &w.failures{body.spawn(k.text(f,size::DETAIL,WARN,0));}
}
fn decision(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,s:&Study,kind:&str,index:usize,enabled:bool){
    let notes=s.refinement_evidence.decisions.iter().rev().find(|d|d.kind==kind&&d.index==index).map(|d|d.notes.clone()).unwrap_or_default();
    for d in ["reviewed","investigating","rejected"]{apply(body,k,stamp,&format!("decision:{kind}:{index}:{d}"),&format!("Review {kind} {index}: {d}"),Command::SetDecision{kind:kind.into(),index,decision:d.into(),notes:notes.clone()},enabled);}
    field(body,k,ui,stamp,Field::RefineDecision(kind.into(),index),"Retained review notes (review only; candidate use is explicit)",notes,enabled);
}

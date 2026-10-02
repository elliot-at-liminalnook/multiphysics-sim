//! Native electrical controls submit the same stamped Study actions as REST.
//! Presentation only: no disk reads, validation execution, simulation or owner.
use super::{button,field};
use super::super::{StudyOwner,StudyAction,StudyStamp,forms::{StudyUi,Field,electrical_forms}};
use bevy::prelude::ChildSpawnerCommands;
use crate::ui_kit::{Kit,size,TEXT,FAINT,WARN};
use sim_runtime::{experiment_study::{Study,refinement::{Command,Operation,electrical}},controller_refinement::{power,recording::Purpose}};
use serde_json::Value;
fn apply(body:&mut ChildSpawnerCommands,k:&Kit,stamp:StudyStamp,id:String,label:&str,command:electrical::Command,enabled:bool){
    button(body,k,id,label,StudyAction::RefineApply{stamp,command:Command::Electrical(command)},enabled);
}
fn scalar(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,path:String,label:&str,value:&Value,enabled:bool){
    let text=match value{Value::String(s)=>s.clone(),Value::Null=>String::new(),_=>value.to_string()};
    field(body,k,ui,stamp,Field::Electrical(path),label,text,enabled);
}
const LIMITS:[(&str,&str);6]=[("minimum_voltage_v","Minimum supply voltage [V]"),("maximum_discharge_current_a","Maximum discharge current [A]"),("maximum_charge_current_a","Maximum charge current [A]"),("maximum_winding_current_a","Maximum winding current magnitude [A]"),("maximum_draw_power_w","Maximum draw power [W]"),("maximum_return_power_w","Maximum returned power [W]")];
fn limits(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,group:&str,value:&Value,enabled:bool){
    for(name,label)in LIMITS{scalar(body,k,ui,stamp,format!("{group}:/limits/{name}"),&format!("{label} · blank: undeclared / unscored"),value.pointer(&format!("/limits/{name}")).unwrap_or(&Value::Null),enabled);}
}
pub(crate) fn section(body:&mut ChildSpawnerCommands,k:&Kit,owner:&StudyOwner,ui:&StudyUi,stamp:StudyStamp,s:&Study,usable:bool){
    body.spawn(k.section("Electrical source and controller feedback"));
    body.spawn(k.caption("Offline hypotheses; review never promotes source properties, registry models or CAD. Source and winding current are different circuit quantities."));
    field(body,k,ui,stamp,Field::Electrical("voltage".into()),"Imposed bus voltage [V] · blank: source owns voltage",s.draft.conditions.voltage_v.map(|v|v.to_string()).unwrap_or_default(),usable);
    for(label,battery)in[("Add regulated source",false),("Add illustrative battery",true)]{
        apply(body,k,stamp,format!("study:electrical:preset:{battery}"),label,electrical::Command::SetSource(Some(electrical_forms::source_preset(s,battery))),usable);
    }
    apply(body,k,stamp,"study:electrical:source:remove".into(),"Remove electrical source",electrical::Command::SetSource(None),usable&&s.draft.power.is_some());
    if let Some(p)=&s.draft.power{
        if let Ok(v)=serde_json::to_value(p){
            scalar(body,k,ui,stamp,"source:/source_component".into(),"Registry source component ID",&v["source_component"],usable);
            let registry=sim_runtime::registry();let descriptor=registry.get(&p.source_component.as_str().into()).ok();
            for(name,value)in &p.source_parameters{
                let declaration=descriptor.as_ref().and_then(|d|d.parameters.as_ref()).and_then(|params|params.iter().find(|d|d.name==*name));
                let label=declaration.map(|d|format!("{name} [{}] · registry range {:?}–{:?} · required {} · default {:?} ({:?})",d.unit,d.minimum,d.maximum,d.required,d.default,d.default_label)).unwrap_or_else(||format!("{name} · retained parameter; registry metadata unavailable"));
                let escaped=name.replace('~',"~0").replace('/',"~1");
                scalar(body,k,ui,stamp,format!("source:/source_parameters/{escaped}"),&label,&serde_json::json!(value),usable);
            }
            scalar(body,k,ui,stamp,"source:/auxiliary_current_a".into(),"Auxiliary electronics consumption [A] · explicit approximation",&v["auxiliary_current_a"],usable);
            scalar(body,k,ui,stamp,"source:/evidence".into(),"Source assumptions / measurement provenance",&v["evidence"],usable);
            limits(body,k,ui,stamp,"source",&v,usable);
            field(body,k,ui,stamp,Field::Electrical("source_json".into()),"Full source definition · registry parameters / retained content",serde_json::to_string_pretty(p).unwrap_or_default(),usable);
        }
    }
    apply(body,k,stamp,"study:electrical:controller:enable".into(),"Enable hypothetical voltage feedback",electrical::Command::SetController(Some(electrical_forms::controller_preset())),usable&&s.refinement.experiment.electrical.is_none());
    apply(body,k,stamp,"study:electrical:controller:remove".into(),"Disable electrical feedback",electrical::Command::SetController(None),usable&&s.refinement.experiment.electrical.is_some());
    if let Some(c)=&s.refinement.experiment.electrical{if let Ok(v)=serde_json::to_value(c){
        for(pointer,label)in[("/sensing/voltage_quantum_v","Voltage resolution [V]"),("/sensing/supply_current_quantum_a","Supply current resolution [A] · blank: unavailable"),("/sensing/winding_current_quantum_a","Winding current resolution [A] · blank: unavailable"),("/sensing/evidence","Sensing evidence / uncertainty / sample age"),("/nominal_voltage_for_compensation_v","Nominal PWM compensation voltage [V] · blank: disabled")]{scalar(body,k,ui,stamp,format!("controller:{pointer}"),label,v.pointer(pointer).unwrap_or(&Value::Null),usable);}
        body.spawn(k.caption("Sampled controller protection: violations command zero PWM subject to the captured command delay. Zero PWM is not a battery disconnect. Current channels remain hypotheses until independently calibrated."));
        limits(body,k,ui,stamp,"controller",&v,usable);
        field(body,k,ui,stamp,Field::Electrical("controller_json".into()),"Full feedback definition",serde_json::to_string_pretty(c).unwrap_or_default(),usable);
    }}
    button(body,k,"study:electrical:simulate","Simulate captured electrical source and feedback",StudyAction::RefineRun{stamp,operation:Operation::Simulate},usable&&!owner.busy()&&s.draft.power.is_some());
    body.spawn(k.section("Electrical recording and prediction selection"));
    for(hash,r)in s.refinement_evidence.recording_identities.iter().zip(&s.refinement.recordings){
        button(body,k,format!("study:electrical:recording:{hash}"),&format!("Select capture {} · device {} · {}",r.experiment.name,r.experiment.device,if r.completed{"complete"}else{"INCOMPLETE / UNSCORED"}),StudyAction::RefineApply{stamp,command:Command::SelectRecording{recording_hash:hash.clone()}},usable);
    }
    if let Some(hash)=s.refinement_evidence.selected_recording.as_ref(){
        for purpose in[Purpose::RecordedCommandReplay,Purpose::ClosedLoopPrediction]{button(body,k,format!("study:electrical:predict:{purpose:?}"),&format!("Predict {purpose:?} · capture immutable electrical inputs"),StudyAction::RefineRun{stamp,operation:Operation::PredictRecording{recording_hash:hash.clone(),purpose}},usable&&!owner.busy());}
    }
    for(i,p)in s.refinement.predictions.iter().enumerate().filter(|(_,p)|p.electrical.is_some()){
        apply(body,k,stamp,format!("study:electrical:prediction:{i}"),&format!("Inspect prediction {i} · {:?} · source {}",p.purpose,p.recording_hash),electrical::Command::SelectPrediction(i),usable);
    }
    if let Some(i)=s.refinement_evidence.electrical.selected_prediction{if let Some(p)=s.refinement.predictions.get(i){
        button(body,k,"study:electrical:compare:servo","Compare captured servo voltage",StudyAction::RefineRun{stamp,operation:Operation::Electrical(electrical::Operation::CompareServoVoltage{recording_hash:p.recording_hash.clone(),prediction:i})},usable&&!owner.busy());
        field(body,k,ui,stamp,Field::Electrical("import".into()),"Calibrated sidecar JSON path · Enter imports and compares in retained job",String::new(),usable&&!owner.busy());
        body.spawn(k.caption(format!("Captured prediction runtime {} · model {} · {:?} · assumptions {}",p.runtime.library_source_blake3,p.model.fingerprint(),p.purpose,p.assumptions)));
        if let Some(trace)=&p.electrical{trace_review(body,k,trace,true);}
    }}
    review(body,k,ui,stamp,s,usable);
}
fn trace_review(body:&mut ChildSpawnerCommands,k:&Kit,t:&power::Trace,complete:bool){
    let summary=&t.summary;
    body.spawn(k.caption(&t.interpretation));
    body.spawn(k.text(format!("Electrical acceptance {} · voltage {}–{} V · sampled discharge / charge / winding peaks {} / {} / {} A · sampled draw / return peaks {} / {} W · energy drawn / returned {} / {} J",if !complete{"UNSCORED incomplete / failed / cancelled"}else{match summary.passes{Some(true)=>"passes declared limits",Some(false)=>"fails declared limits",None=>"UNSCORED no declared limits"}},summary.minimum_voltage_v,summary.maximum_voltage_v,summary.peak_discharge_current_a,summary.peak_charge_current_a,summary.peak_winding_current_a,summary.peak_draw_power_w,summary.peak_return_power_w,summary.drawn_energy_j,summary.returned_energy_j),size::DETAIL,if complete{TEXT}else{WARN},0));
    body.spawn(k.caption(format!("Captured electrical limits {:?} · violations {:?}. Peaks are sampled, not switching peaks; watts and joules use same-circuit synchronized voltage/current.",t.limits,summary.violations)));
    body.spawn(k.caption(format!("{} samples retained; first 16 shown with exact observation times. Full traces remain in immutable evidence.",t.samples.len())));
    for v in t.samples.iter().take(16){body.spawn(k.mono(format!("t {:.6} s · supply {:.6} V / {:.6} A / {:.6} W · winding {:.6} V / {:.6} A / {:.6} W · SOC {:?}",v.time_s,v.supply_voltage_v,v.supply_current_a,v.supply_power_w,v.winding_voltage_v,v.winding_current_a,v.winding_power_w,v.state_of_charge),size::DETAIL,FAINT));}
}
fn review(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,s:&Study,usable:bool){
    body.spawn(k.section("Captured electrical review · separate from motion tracking"));
    for(i,r)in s.refinement.controller_runs.iter().enumerate().filter(|(_,r)|r.electrical.is_some()){
        button(body,k,format!("study:electrical:run:{i}"),&format!("Select captured electrical run {i}"),StudyAction::RefineApply{stamp,command:Command::SelectControllerRun(i)},usable);
        body.spawn(k.caption(format!("Run {i} · runtime {} · captured feedback {:?} · command delay {} s · cancellation {} · failure {:?} · {}",r.runtime.library_source_blake3,r.experiment.electrical,r.experiment.timing.command_delay_ticks as f64*r.experiment.timing.period_s,r.cancelled,r.failure,if r.model==s.draft&&r.experiment==s.refinement.experiment{"captured inputs match draft"}else{"STALE to current draft"})));
        if let Some(t)=&r.electrical{trace_review(body,k,t,!r.cancelled&&r.failure.is_none());}
        for f in r.frames.iter().filter(|f|!f.electrical_limit_reasons.is_empty()).take(16){body.spawn(k.mono(format!("Sampled protection t {:.6} s · observed {:.6} s · received {:.6} s · channels {:?} · reasons {:?} · requested / applied PWM {} / {}",f.time_s,f.observation.observed_s,f.observation.received_s,f.observation.electrical,f.electrical_limit_reasons,f.requested_duty,f.applied_duty),size::DETAIL,WARN));}
    }
    body.spawn(k.caption("Servo current registers remain raw uncalibrated counts; they are not battery or winding amps, watts or joules. Only calibrated synchronized channels produce measured power. Missing channels and limits remain unscored."));
    for(i,e)in s.refinement.electrical_comparisons.iter().enumerate(){
        apply(body,k,stamp,format!("study:electrical:comparison:{i}"),&format!("Select immutable electrical comparison {i}"),electrical::Command::SelectComparison(i),usable);
        body.spawn(k.caption(format!("Comparison {i} · prediction {} · recording {} · measurement schema {} · sources {:?} · timing {} · method {}",e.prediction_hash,e.measurements.recording_hash,e.measurements.version,e.measurements.source_hashes,e.measurements.timing_evidence,e.method)));
        for c in &e.measurements.channels{
            body.spawn(k.caption(format!("Measured {} · sensor {} · circuit {} · raw unit {} · polarity/gain {} · offset {} · evidence {} · uncertainty {}",c.name,c.calibration.sensor,c.calibration.circuit_location,c.calibration.raw_unit,c.calibration.gain,c.calibration.offset,c.calibration.evidence,c.calibration.uncertainty)));
            for v in c.raw_samples.iter().take(8){body.spawn(k.mono(format!("Raw {} t {} s · observation window {}–{} s · {} {}",c.name,v.time_s,v.request_s,v.completion_s,v.value,c.calibration.raw_unit),size::DETAIL,FAINT));}
        }
        for c in &e.channels{
            body.spawn(k.text(format!("{} · RMSE {} {} · maximum absolute error {} · final error {} · {} · captured thresholds {:?}",c.name,c.rmse,c.measured.unit,c.maximum_abs_error,c.final_error,match c.passes{Some(true)=>"passes captured channel limits",Some(false)=>"fails captured channel limits",None=>"UNSCORED no channel limits"},e.measurements.limits.get(&c.name)),size::DETAIL,TEXT,0));
            for(label,t)in[("measured / derived",&c.measured),("hypothetical prediction",&c.predicted)]{for v in t.samples.iter().take(8){body.spawn(k.mono(format!("{label} {} t {} s · window {}–{} s · {} {}",c.name,v.time_s,v.request_s,v.completion_s,v.value,t.unit),size::DETAIL,FAINT));}}
        }
        if let Some(v)=&e.supply_energy{body.spawn(k.caption(format!("Synchronized derived supply energy {}–{} s · drawn measured / predicted {} / {} J · returned {} / {} J",v.start_s,v.end_s,v.measured_drawn_j,v.predicted_drawn_j,v.measured_returned_j,v.predicted_returned_j)));}else{body.spawn(k.caption("Supply energy UNSCORED: synchronized supply voltage/current unavailable."));}
        let notes=s.refinement_evidence.decisions.iter().rev().find(|d|d.kind=="electrical"&&d.index==i).map(|d|d.notes.clone()).unwrap_or_default();
        field(body,k,ui,stamp,Field::RefineDecision("electrical".into(),i),"Electrical review notes · no physical property promotion",notes.clone(),usable);
        for decision in["reviewed","investigating","rejected"]{button(body,k,format!("study:electrical:decision:{i}:{decision}"),decision,StudyAction::RefineApply{stamp,command:Command::SetDecision{kind:"electrical".into(),index:i,decision:decision.into(),notes:notes.clone()}},usable);}
    }
}

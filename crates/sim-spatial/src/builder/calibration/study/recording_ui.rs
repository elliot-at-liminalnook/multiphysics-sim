//! Transient kit children belong to the existing dock, which tears them down.
//! StudyOwner is the single durable writer; StudyUi owns retained raw drafts.
//! Input emits occurrences in ViewerSet::Input; Actions validates and jobs own
//! disk/physics. Present renders captured evidence, never executes predictions.
use super::{button,field};
use super::super::{StudyOwner,StudyAction,StudyStamp,forms::{StudyUi,Field,Hit,recording_forms}};
use bevy::prelude::*;
use crate::ui_kit::{Kit,Look,size,TEXT,FAINT,WARN};
use sim_runtime::{experiment_study::{Study,refinement::{Command,Operation}},controller_refinement::{recording::Purpose,context::{Property,Origin}}};
fn apply(body:&mut ChildSpawnerCommands,k:&Kit,stamp:StudyStamp,id:String,label:&str,command:Command,enabled:bool){button(body,k,id,label,StudyAction::RefineApply{stamp,command},enabled)}
fn scalar(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,hash:&str,pointer:&str,label:&str,value:String,enabled:bool){field(body,k,ui,stamp,Field::Recording(format!("{hash}:{pointer}")),label,value,enabled)}
pub(crate) fn section(body:&mut ChildSpawnerCommands,k:&Kit,owner:&StudyOwner,ui:&StudyUi,stamp:StudyStamp,s:&Study,usable:bool){
    body.spawn(k.section("Controller recording capture and setup"));
    for name in ["native_recording_imports","native_refinement_rejections"]{
        if let Some(rows)=s.retained_fields.get(name).and_then(serde_json::Value::as_array){for row in rows{
            body.spawn(k.text(format!("Retained {name} · stamp {} · source {} · classification {} · applied {} · stale {} · displaced {} · error {}",row.get("stamp").unwrap_or(&serde_json::Value::Null),row.get("source").unwrap_or(&serde_json::Value::Null),row.pointer("/input/classification").unwrap_or(&serde_json::Value::Null),row.get("applied").unwrap_or(&serde_json::Value::Null),row.get("stale").unwrap_or(&serde_json::Value::Null),row.get("displaced").unwrap_or(&serde_json::Value::Null),row.get("error").unwrap_or(&serde_json::Value::Null)),size::DETAIL,WARN,0));
            if let Some(raw)=row.pointer("/input/raw").and_then(serde_json::Value::as_str){body.spawn(k.mono(raw.chars().take(4096).collect::<String>(),size::DETAIL,FAINT));if raw.chars().nth(4096).is_some(){body.spawn(k.caption("Raw input preview truncated at 4096 characters; full exact captured input remains in immutable saved review evidence."));}}
            if let Some(action)=row.get("action"){body.spawn(k.caption(format!("Rejected action operation {} · captured stamp {}",action.get("op").unwrap_or(&serde_json::Value::Null),action.get("stamp").unwrap_or(&serde_json::Value::Null))));}
        }}
    }
    field(body,k,ui,stamp,Field::Recording("import".into()),"Controller recording file · Enter imports in a job; FPGA captures use the deferred FPGA workflow",String::new(),usable&&!owner.busy());
    for (hash,r) in s.refinement_evidence.recording_identities.iter().zip(&s.refinement.recordings) {apply(body,k,stamp,format!("study:recording:select:{hash}"),&format!("Inspect {} · device {} · {}",r.experiment.name,r.experiment.device,if r.completed{"complete"}else{"INCOMPLETE / UNSCORED"}),Command::SelectRecording{recording_hash:hash.clone()},usable);}
    if let Some(hash)=s.refinement_evidence.selected_recording.as_deref(){if let Some(r)=sim_runtime::experiment_study::refinement::recordings::cached_source(s,hash){
        body.spawn(k.caption(format!("Source {hash} · schema {} · runtime {} · source hashes {:?} · completed {} · verified stop {} · failure {:?}",r.version,r.runtime.library_source_blake3,r.source_hashes,r.completed,r.stop_verified,r.failure)));
        body.spawn(k.caption(format!("Captured experiment {:?} · host origin {} s · stop {}–{} s · timing {}",r.experiment,r.transactions_origin_host_s,r.stop_request_s,r.stop_receipt_s,r.timing_evidence)));
        body.spawn(k.caption("Setup edits append immutable inspection revisions; captured physical facts remain unchanged. Unknown values stay explicit. Property value entry declares an estimate; choose measured/derived only with independent source evidence."));
        for (revision,c) in s.refinement.capture_contexts.iter().enumerate().filter(|(_,c)|c.recording_hash==hash){
            body.spawn(k.caption(format!("Immutable setup revision {revision} · source {} · fixture {} · attached {} · transmission {}",c.recording_hash,c.fixture,c.attached_output_hardware,c.transmission)));
            for p in &c.properties{body.spawn(k.caption(format!("Captured property {} = {:?} [{}] · frame {} · {:?} · source {} · uncertainty {:?}",p.name,p.value,p.unit,p.coordinate_frame,p.origin,p.source,p.uncertainty_bounds)));}
            body.spawn(k.caption(format!("Revision artifacts {:?} · bindings {:?} · limitations {:?}",c.artifacts,c.bindings,c.limitations)));
        }
        if let Ok(c)=recording_forms::context(s,hash){
            for (pointer,label,value) in [("/fixture","Fixture",c.fixture.clone()),("/attached_output_hardware","Attached output hardware",c.attached_output_hardware.clone()),("/transmission","Transmission",c.transmission.clone())]{scalar(body,k,ui,stamp,hash,pointer,label,value,usable);}
            for (i,p) in c.properties.iter().enumerate(){
                for (name,label,value) in [("name","Property name",p.name.clone()),("value","Estimated property value · blank: unknown",p.value.map(|v|v.to_string()).unwrap_or_default()),("unit","Declared unit",p.unit.clone()),("coordinate_frame","Coordinate frame",p.coordinate_frame.clone()),("source","Provenance source",p.source.clone())]{scalar(body,k,ui,stamp,hash,&format!("/properties/{i}/{name}"),label,value,usable);}
                body.spawn(k.caption(format!("Origin {:?}",p.origin)));
                scalar(body,k,ui,stamp,hash,&format!("/properties/{i}/uncertainty_bounds"),"Uncertainty absolute bounds · lower, upper · blank: unestablished",p.uncertainty_bounds.map(|[lo,hi]|format!("{lo}, {hi}")).unwrap_or_default(),usable);
                for origin in [Origin::Measured,Origin::Derived,Origin::Estimated]{let provenance=matches!(origin,Origin::Estimated)||(!p.source.contains("No independent measurement supplied")&&!p.source.contains("not established"));let mut next=c.clone();next.properties[i].origin=origin.clone();apply(body,k,stamp,format!("study:recording:origin:{hash}:{i}:{origin:?}"),&format!("Label property {i} {origin:?}"),Command::AppendContext{context:next},usable&&p.value.is_some()&&provenance);}
                let mut next=c.clone();next.properties.remove(i);apply(body,k,stamp,format!("study:recording:property:remove:{hash}:{i}"),"Remove property from new setup revision",Command::AppendContext{context:next},usable);
            }
            let mut next=c.clone();let mut n=next.properties.len();while next.properties.iter().any(|p|p.name==format!("property_{n}")){n+=1;}
            next.properties.push(Property{name:format!("property_{n}"),value:None,unit:"unit not established".into(),coordinate_frame:"frame not established".into(),origin:Origin::Unknown,source:"source not established".into(),uncertainty_bounds:None});
            apply(body,k,stamp,format!("study:recording:property:add:{hash}"),"Add unknown setup property",Command::AppendContext{context:next},usable);
            for (i,a) in c.artifacts.iter().enumerate(){for (name,label,value) in [("role","Artifact role",a.role.clone()),("location","Durable artifact location",a.location.clone()),("blake3","Artifact content BLAKE3",a.blake3.clone())]{scalar(body,k,ui,stamp,hash,&format!("/artifacts/{i}/{name}"),label,value,usable);}let mut next=c.clone();next.artifacts.remove(i);apply(body,k,stamp,format!("study:recording:artifact:remove:{hash}:{i}"),"Remove artifact from new setup revision",Command::AppendContext{context:next},usable);}
            for (i,b) in c.bindings.iter().enumerate(){for (name,label,value) in [("hardware_id","Hardware device ID",b.hardware_id.to_string()),("cad_component_id","Stable CAD component ID",b.cad_component_id.clone()),("joint_id","Joint ID · blank: absent",b.joint_id.clone().unwrap_or_default()),("source","Binding provenance",b.source.clone())]{scalar(body,k,ui,stamp,hash,&format!("/bindings/{i}/{name}"),label,value,usable);}let mut next=c.clone();next.bindings.remove(i);apply(body,k,stamp,format!("study:recording:binding:remove:{hash}:{i}"),"Remove binding from new setup revision",Command::AppendContext{context:next},usable);}
            for (kind,fields) in [("artifact",vec![("role","Artifact role"),("location","Durable artifact location"),("blake3","Artifact BLAKE3 content hash")]),("binding",vec![("hardware_id","Hardware device ID"),("cad_component_id","Stable CAD component ID"),("joint_id","Joint ID · optional"),("source","Binding provenance")]),("limitation",vec![("text","Setup limitation / uncertainty")])]{
                for (name,label) in fields{scalar(body,k,ui,stamp,hash,&format!("new/{kind}/{name}"),label,String::new(),usable);}
                body.spawn(k.button(&format!("Append {kind} row to immutable setup revision"),Hit::AddRecordingRow{stamp,hash:hash.into(),kind:kind.into()},Look::Secondary,usable));
            }
            for (i,limit) in c.limitations.iter().enumerate(){scalar(body,k,ui,stamp,hash,&format!("/limitations/{i}"),"Setup limitation",limit.clone(),usable);let mut next=c.clone();next.limitations.remove(i);apply(body,k,stamp,format!("study:recording:limitation:remove:{hash}:{i}"),"Remove limitation from new setup revision",Command::AppendContext{context:next},usable);}
        }
        if let Some(a)=s.refinement.recording_assignments.iter().find(|a|a.recording_hash==hash){body.spawn(k.text(format!("FROZEN whole run · {:?} · limits {:?} · rationale {}",a.role,a.limits,a.rationale),size::DETAIL,TEXT,0));}else{
            for (name,label,value) in [("role","Whole-run role · train or held_out","held_out".into()),("rmse","RMSE limit [rad]",r.experiment.limits.rms_rad.to_string()),("final_abs_error","Final absolute error limit [rad]",r.experiment.limits.settled_rad.to_string()),("rationale","Reservation rationale",String::new())]{scalar(body,k,ui,stamp,hash,&format!("assignment/{name}"),label,value,usable);}
            body.spawn(k.button("Freeze whole run role, limits and rationale",Hit::FreezeRecording{stamp,hash:hash.into()},Look::Secondary,usable&&r.completed&&r.failure.is_none()&&r.stop_verified));
        }
        for purpose in [Purpose::RecordedCommandReplay,Purpose::ClosedLoopPrediction]{
            let label=match purpose{Purpose::RecordedCommandReplay=>"Recorded-command replay · apply captured commands",Purpose::ClosedLoopPrediction=>"Own-feedback closed-loop prediction · controller sees simulated feedback"};
            apply(body,k,stamp,format!("study:recording:purpose:{purpose:?}"),label,Command::SetPredictionPurpose(purpose),usable);
        }
        let purpose=s.refinement_evidence.prediction_purpose.unwrap_or(Purpose::RecordedCommandReplay);
        body.spawn(k.caption(format!("Selected purpose {purpose:?}. Incomplete captures are inspectable and cannot be scored or fitted.")));
        button(body,k,"study:recording:predict","Predict selected capture",StudyAction::RefineRun{stamp,operation:Operation::PredictRecording{recording_hash:hash.into(),purpose}},usable&&!owner.busy()&&r.completed&&r.failure.is_none()&&r.stop_verified);
        for f in r.frames.iter().take(12){body.spawn(k.mono(format!("Captured t {:.6} s · observation {:.6} s · angle {:.6} rad · duty {:.6} · command {}–{} s",f.control.time_s,f.control.observation.observed_s,f.control.estimated_position_rad,f.control.applied_duty,f.command_request_s,f.command_receipt_s),size::DETAIL,FAINT));}
    }}
    body.spawn(k.caption("Held-out whole-run reservations never become tuning data; all prior exposure and influence remain retained. Fits do not accept a registry or CAD model."));
    button(body,k,"study:recording:fit","Fit frozen recording assignments",StudyAction::RefineRun{stamp,operation:Operation::FitRecordings},usable&&!owner.busy());
    field(body,k,ui,stamp,Field::Recording("additional".into()),"Optional additional saved study path · Enter loads and fits; blank uses current archive + recordings",String::new(),usable&&!owner.busy());
    button(body,k,"study:recording:combined","Fit current archive + frozen recordings",StudyAction::FitCombined{stamp,additional_path:None},usable&&!owner.busy());
    if ui.recording_charts.current(owner){
        body.spawn(k.caption(&ui.recording_charts.label));
        body.spawn(k.caption(if s.refinement_evidence.selected_fit_case.is_some(){"Trace colors: measured teal, baseline orange, candidate blue; each series keeps its original sample times."}else{"Trace colors: measured teal, standalone prediction orange; each series keeps its original sample times."}));
        body.spawn(k.caption(format!("Captured-time angle [rad] · time {}–{} s · angle {}–{} rad",ui.recording_charts.axes.1.0,ui.recording_charts.axes.1.1,ui.recording_charts.axes.0.0,ui.recording_charts.axes.0.1)));
        if let Some(image)=&ui.recording_charts.image{body.spawn(k.chart_image(image.clone(),Node{width:Val::Percent(100.),height:Val::Px(180.),..default()},true));}
    }
    if let Some(error)=&ui.recording_charts.error{body.spawn(k.text(error,size::DETAIL,WARN,0));}
    apply(body,k,stamp,"study:recording:case:clear".into(),"Return chart to selected recording / standalone prediction",Command::SelectFitCase{selection:None},usable);
    review(body,k,ui,stamp,s,usable);
}
fn review(body:&mut ChildSpawnerCommands,k:&Kit,ui:&StudyUi,stamp:StudyStamp,s:&Study,usable:bool){
    body.spawn(k.section("Captured recording predictions and fit review"));
    for p in &s.refinement.predictions{
        body.spawn(k.text(format!("{:?} · source {} · model {} · {} · runtime {}",p.purpose,p.recording_hash,p.model.fingerprint(),if p.model==s.draft{"captured model matches draft"}else{"STALE model"},p.runtime.library_source_blake3),size::DETAIL,TEXT,0));
        body.spawn(k.caption(&p.assumptions));if p.runtime!=sim_runtime::physics_context::RuntimeIdentity::current(){body.spawn(k.text("STALE runtime source/features; captured evidence remains labelled with its runtime",size::DETAIL,WARN,0));}
        body.spawn(k.caption(format!("Captured limits {:?} · measured tracking {:?} · simulated tracking {:?} · model error {:?}",p.limits,p.measured_tracking,p.simulated_tracking,(p.model_error.rmse,p.model_error.final_error,p.model_error.maximum_abs_error,p.model_error.passes))));
        body.spawn(k.caption("Measured and predicted samples below retain their captured observation timestamps [s, rad]; controller and physics timing assumptions are retained above."));
        for (label,t) in [("measured",&p.measured),("predicted",&p.predicted)]{for point in t.samples.iter().take(12){body.spawn(k.mono(format!("{label} t {:.6} s · {:.6} rad",point.time_s,point.value),size::DETAIL,FAINT));}}
    }
    for (kind,attempts) in [("recording_fit",s.refinement.recording_fits.iter().map(|f|(&f.attempt,format!("assignments {:?}",f.dataset.assignments))).collect::<Vec<_>>()),("combined_fit",s.refinement.combined_fits.iter().map(|f|(&f.attempt,format!("archives {:?} · assignments {:?}",f.dataset.archives.iter().map(|a|(&a.label,&a.observation_blake3,&a.model_blake3)).collect::<Vec<_>>(),f.dataset.recordings.as_ref().map(|d|&d.assignments)))).collect::<Vec<_>>())]{for (i,(a,dataset)) in attempts.into_iter().enumerate(){
        body.spawn(k.text(format!("{kind} {i} · cancelled {} · failure {:?} · captured archive {} · runtime {}",a.cancelled,a.failure,a.archive_hash,a.runtime.library_source_blake3),size::DETAIL,if a.cancelled||a.failure.is_some(){WARN}else{TEXT},0));
        body.spawn(k.caption(if a.request.model.shared==s.draft{"Captured fit model matches current draft"}else{"STALE fit model: current draft differs from captured request"}));
        if a.runtime!=sim_runtime::physics_context::RuntimeIdentity::current(){body.spawn(k.text("STALE captured fit runtime source/features",size::DETAIL,WARN,0));}
        body.spawn(k.caption(format!("Frozen dataset {dataset} · request {:?}",a.request)));
        body.spawn(k.caption(format!("Captured objective history: {} evaluations; first 32 shown, full history remains in immutable evidence",a.evaluations.len())));
        for (step,e) in a.evaluations.iter().take(32).enumerate(){body.spawn(k.caption(format!("Evaluation {step} · parameters {:?} · residual sum squares {:?} · failure {:?}",e.values,e.residual_sum_squares,e.failure)));}
        if let Some(partial)=&a.partial{body.spawn(k.text(format!("UNSCORED partial evidence · optimizer {:?} · scores {:?}",optimizer_summary(&partial.optimizer),partial.scores.iter().map(|v|(&v.id,v.device,&v.split,v.baseline.as_ref().map(|c|(c.rmse,c.final_error,c.maximum_abs_error,c.passes)),v.candidate.as_ref().map(|c|(c.rmse,c.final_error,c.maximum_abs_error,c.passes)),&v.failure)).collect::<Vec<_>>()),size::DETAIL,WARN,0));}
        if let Some(identity)=sim_runtime::experiment_study::refinement::recordings::fit_identity(s,kind,i){
            if let Ok(cases)=sim_runtime::experiment_study::refinement::recordings::fit_case_ids(s,kind,i){for case_id in cases{
                let selection=sim_runtime::experiment_study::refinement::recordings::FitCaseSelection{kind:kind.into(),index:i,fit_blake3:identity.into(),case_id:case_id.clone()};
                let selected=s.refinement_evidence.selected_fit_case.as_ref()==Some(&selection);
                apply(body,k,stamp,format!("study:recording:case:{kind}:{i}:{case_id}"),&format!("{}Inspect immutable case {case_id} · measured / baseline / candidate",if selected{"Selected · "}else{""}),Command::SelectFitCase{selection:Some(selection)},usable);
            }}
        }
        let complete=a.outcome.as_ref().is_some_and(|f|f.has_verified_traces()&&!f.training_ids.iter().any(|id|s.refinement_evidence.recording_held_out.contains(id))&&!f.scores.is_empty()&&f.scores.iter().any(|v|v.device==s.refinement.experiment.device)&&f.scores.iter().all(|v|v.failure.is_none()&&v.baseline.is_some()&&v.candidate.is_some()))&&!a.cancelled&&a.failure.is_none();
        if let Some(f)=&a.outcome{body.spawn(k.caption(format!("Outcome {} · optimizer {:?} · frozen tuning {:?} · held-out {:?} · influence {} · scores {:?}",f.status,optimizer_summary(&f.optimizer),f.training_ids,f.validation_ids,f.validation_influenced,f.scores.iter().map(|v|(&v.id,v.device,&v.split,v.baseline.as_ref().map(|c|(c.rmse,c.final_error,c.maximum_abs_error,c.passes)),v.candidate.as_ref().map(|c|(c.rmse,c.final_error,c.maximum_abs_error,c.passes)),&v.failure)).collect::<Vec<_>>())));}
        if !complete{body.spawn(k.text("UNSCORED / unusable candidate: failure, cancellation or incomplete captured comparisons",size::DETAIL,WARN,0));}
        apply(body,k,stamp,format!("study:recording:use:{kind}:{i}"),"Explicit exploratory candidate use · shared identity and device guard",Command::UseRecordingFit{kind:kind.into(),index:i,device:Some(s.refinement.experiment.device)},usable&&complete);
        let notes=s.refinement_evidence.decisions.iter().rev().find(|d|d.kind==kind&&d.index==i).map(|d|d.notes.clone()).unwrap_or_default();
        field(body,k,ui,stamp,Field::RefineDecision(kind.into(),i),"Retained fit review notes",notes.clone(),usable);
        for decision in ["reviewed","investigating","rejected"]{apply(body,k,stamp,format!("study:recording:decision:{kind}:{i}:{decision}"),decision,Command::SetDecision{kind:kind.into(),index:i,decision:decision.into(),notes:notes.clone()},usable);}
    }}
}

/// Optimizer residual vectors can contain every captured sample. Display bounded
/// metrics rather than formatting those vectors on a presentation frame.
fn optimizer_summary(value:&serde_json::Value)->String{
    let get=|key:&str|value.get(key).unwrap_or(&serde_json::Value::Null);
    format!("initial cost {} · final cost {} · evaluations {} · rejected {} · termination {} · iterations {}",get("initial_cost"),get("cost"),get("evaluations"),get("rejected_evaluations"),get("termination"),value.get("history").and_then(serde_json::Value::as_array).map_or(0,Vec::len))
}

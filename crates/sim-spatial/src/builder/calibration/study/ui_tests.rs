//! Written windowless fixtures inspect actual spawned kit entities. Unexecuted.
use super::*;
use bevy::ecs::system::RunSystemOnce;
use super::super::{state::RetainedStudy,jobs::{PendingJob,JobKind,JobOutput}};
use crate::jobs::Job;
use std::sync::atomic::AtomicBool;

fn fixture()->sim_runtime::experiment_study::Study {
    let root=crate::workspace::root().unwrap();
    sim_runtime::experiment_study::Study::new(sim_runtime::experiment_comparison::hx_archive::load(&root.join(super::super::super::DEFAULT_ARCHIVE),root).unwrap()).unwrap()
}
fn fonts()->UiFonts {UiFonts{regular:default(),medium:default(),semibold:default(),italic:default(),mono:default(),icons:BTreeMap::new()}}
fn owner()->StudyOwner {
    let study=fixture();
    let mut owner=StudyOwner::default();
    owner.studies.push(RetainedStudy{id:11,revision:4,saved_revision:None,study,document:None,source:"fixture archive".into(),displaced:None});
    owner.active=Some(11);owner
}
fn pending(owner:&mut StudyOwner) {
    owner.pending.push(PendingJob{id:7,kind:JobKind::Evaluate,stamp:owner.active().map(|r|r.stamp()),document:None,source:"fixture archive".into(),trial_ids:vec![],cancel_requested:false,job:Job::<JobOutput>::finished(7,Err("fixture terminal result".into())),selection_epoch:0,gate:None,captured:owner.active().map(|r|r.study.clone()),launch:Value::Null});
}
fn render(mut commands:Commands,owner:Res<StudyOwner>,ui:Res<StudyUi>) {
    let fonts=fonts();
    commands.spawn(Node::default()).with_children(|body|section(body,&Kit::new(&fonts),&owner,&ui));
}
fn world(owner:StudyOwner)->World {
    let mut world=World::new();world.insert_resource(owner);world.insert_resource(StudyUi::default());world.insert_resource(fonts());world
}
#[test]
fn actual_path_trial_filter_and_publication_controls_reach_stamped_actions() {
    let mut world=world(owner());
    world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    let ui=world.resource::<StudyUi>();
    for field in [Field::Archive,Field::Review,Field::Save,Field::Export] {
        let hits:Vec<_>=ui.rendered.values().filter(|c|matches!(&c.hit,Hit::Path{field:f,..} if *f==field)).collect();
        assert_eq!(hits.len(),1,"actual kit submit button for {field:?}");
        assert!(hits[0].enabled);
    }
    for name in ["device","direction","min_drive","max_drive"] {
        assert!(ui.rendered.values().any(|c|matches!(&c.hit,Hit::Focus{field:Field::Filter(n),..} if *n==name)));
    }
    let Hit::Action(StudyAction::Evaluate{stamp,set:EvaluationSelection::Filtered})=super::super::forms::activate(ui,"study:evaluate:filtered").unwrap() else {panic!("wrong evaluation action")};
    assert_eq!(stamp,StudyStamp{id:11,revision:4});
    assert!(ui.rendered.keys().any(|id|id.starts_with("study:trial:")));
}
#[test]
fn missing_busy_and_displaced_actual_controls_refuse_truthfully() {
    for case in 0..3 {
        let mut owner=owner();
        if case==0 {owner.studies.clear();owner.active=None;}
        if case==1 {pending(&mut owner);}
        if case==2 {owner.studies[0].displaced=Some("same-revision replacement".into());}
        let mut world=world(owner);
        world.run_system_once(render).unwrap();world.run_system_once(status).unwrap();world.run_system_once(collect).unwrap();
        let ui=world.resource::<StudyUi>();
        if case==0 {assert!(!ui.rendered.contains_key("study:evaluate:filtered"));}
        else {assert!(!ui.rendered["study:evaluate:filtered"].enabled);assert!(super::super::forms::activate(ui,"study:evaluate:filtered").is_err());}
        if case==1 {assert!(ui.rendered["study:cancel:7"].enabled);}
        if case==2 {assert!(ui.rendered.values().filter(|c|matches!(&c.hit,Hit::Focus{field:Field::Parameter(..),..})).all(|c|!c.enabled));}
    }
}
#[test]
fn closed_panel_keeps_actual_cancel_access_without_builder_resource() {
    let mut owner=owner();pending(&mut owner);let mut world=world(owner);
    // Only the global renderer runs: there is no dock or Builder resource.
    world.run_system_once(status).unwrap();world.run_system_once(collect).unwrap();
    assert!(world.resource::<StudyUi>().rendered["study:cancel:7"].enabled);
    let mut persistent=world.query_filtered::<Entity,(With<GlobalStatus>,With<crate::app::Persistent>)>();
    assert_eq!(persistent.iter(&world).count(),1);
    assert_eq!(world.resource::<StudyOwner>().pending.len(),1,"presentation never cancels a job");
}
#[test]
fn late_text_keeps_original_identity_and_revision_and_refuses_nonfinite() {
    let mut owner=owner();let stamp=owner.active().unwrap().stamp();
    assert!(super::super::forms::submission(&owner,Some(stamp),&Field::Step,"NaN").unwrap_err().contains("finite"));
    owner.studies[0].revision+=1;
    assert!(super::super::forms::submission(&owner,Some(stamp),&Field::Notes,"late").is_err());
    owner.studies[0].revision=stamp.revision;owner.studies[0].id=22;owner.active=Some(22);
    assert!(super::super::forms::submission(&owner,Some(stamp),&Field::Notes,"late").is_err());
    assert!(owner.studies[0].study.notes.is_empty());
}
#[test]
fn stale_cancelled_captures_render_four_labels_and_unscored_metrics() {
    let mut owner=owner();let s=&mut owner.studies[0].study;
    let ids=vec![s.archive.trials[0].id.clone()];
    let captured=sim_runtime::experiment_study::evaluate(&s.archive,&ids,&s.baseline,&s.draft,None,false,&AtomicBool::new(true),|_,_|{}).unwrap();
    s.evaluations.push(captured);s.view.evaluation=Some(0);s.view.trial_id=Some(ids[0].clone());s.draft.step_s*=0.5;
    let mut world=world(owner);world.run_system_once(render).unwrap();
    let mut texts=world.query::<&Text>();let text=texts.iter(&world).map(|t|t.0.as_str()).collect::<Vec<_>>().join("\n");
    for label in chart::LABELS {assert!(text.contains(label));}
    assert!(text.contains("STALE"));assert!(text.contains("UNSCORED"));assert!(text.contains("0 pass"));
}
#[test]
fn shared_refusal_retains_submitted_text_and_actual_discard_button() {
    let owner=owner();let stamp=owner.active().unwrap().stamp();let mut ui=StudyUi::default();
    let action=super::super::forms::text_submission(&mut ui,&owner,Hit::Focus{stamp:Some(stamp),field:Field::Step,text:"ignored rendered fallback".into()},"0.001").unwrap();
    assert!(ui.blocking_reason().is_some());
    super::super::forms::acknowledge(&mut ui,&action,&sim_api::Outcome::Done(Err("document identity changed".into())));
    assert!(ui.blocking_reason().is_some());assert!(ui.error.as_ref().unwrap().contains("identity"));
    let mut world=world(owner);world.insert_resource(ui);world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    let mut buttons=world.query_filtered::<(&Hit,&Enabled),With<Button>>();
    assert!(buttons.iter(&world).any(|(h,e)|matches!(h,Hit::Discard{..}) && e.0));
    assert!(world.resource::<StudyUi>().drafts.values().any(|t|t=="0.001"));
}
#[test]
fn accepted_request_ack_clears_only_its_field_and_cannot_erase_other_draft() {
    let owner=owner();let stamp=owner.active().unwrap().stamp();let mut ui=StudyUi::default();
    let action=super::super::forms::text_submission(&mut ui,&owner,Hit::Focus{stamp:Some(stamp),field:Field::Notes,text:String::new()},"submitted notes").unwrap();
    let other=((Some((stamp.id,stamp.revision)),Field::Condition("load_inertia")),"unresolved other field".to_string());
    ui.drafts.insert(other.0.clone(),other.1.clone());
    super::super::forms::acknowledge(&mut ui,&action,&sim_api::Outcome::Done(Ok(json!({"accepted":true}))));
    assert_eq!(ui.drafts.len(),1);assert_eq!(ui.drafts[&other.0],other.1);
    assert!(ui.blocking_reason().is_some());
}
#[test]
fn equivalent_pending_conditions_commands_preserve_both_raw_fields() {
    let owner=owner();let r=owner.active().unwrap();let stamp=r.stamp();let mut ui=StudyUi::default();
    let inertia=r.study.draft.conditions.load_inertia.to_string();let torque=r.study.draft.conditions.load_torque.to_string();
    let focus=|name|Hit::Focus{stamp:Some(stamp),field:Field::Condition(name),text:String::new()};
    super::super::forms::text_submission(&mut ui,&owner,focus("load_inertia"),&inertia).unwrap();
    assert!(super::super::forms::text_submission(&mut ui,&owner,focus("load_torque"),&torque).unwrap_err().contains("equivalent"));
    assert_eq!(ui.awaiting.len(),1);assert_eq!(ui.drafts.len(),2);
}
#[test]
fn accepted_ack_never_erases_newer_text_in_the_same_field() {
    let owner=owner();let stamp=owner.active().unwrap().stamp();let mut ui=StudyUi::default();
    let action=super::super::forms::text_submission(&mut ui,&owner,Hit::Focus{stamp:Some(stamp),field:Field::Notes,text:String::new()},"submitted text").unwrap();
    let key=(Some((stamp.id,stamp.revision)),Field::Notes);
    ui.drafts.insert(key.clone(),"typed later in the same frame".into());
    super::super::forms::acknowledge(&mut ui,&action,&sim_api::Outcome::Done(Ok(json!({"accepted":true}))));
    assert_eq!(ui.drafts[&key],"typed later in the same frame");assert!(ui.blocking_reason().is_some());
}

#[derive(Component)]
struct ReconstructedPanel;
#[derive(Resource,Default)]
struct Reconstructions(usize);
/// Uses the same invalidation gate as builder::ui::rebuild_panel and renders
/// the actual study surface, while avoiding unrelated scene/camera setup.
fn rebuild_on_publication(mut commands:Commands,owner:Res<StudyOwner>,ui:Res<StudyUi>,old:Query<Entity,With<ReconstructedPanel>>,mut count:ResMut<Reconstructions>) {
    if !presentation_changed(&owner,&ui){return}
    for entity in &old{commands.entity(entity).despawn();}
    count.0+=1;let fonts=fonts();
    commands.spawn((ReconstructedPanel,Node::default())).with_children(|body|section(body,&Kit::new(&fonts),&owner,&ui));
}
#[test]
fn idle_polling_keeps_actual_panel_entities_and_real_text_terminal_publication_rebuilds() {
    use crate::ui_kit::text::{FieldMsg,FieldEvent,TextDraft};
    let mut studies=owner();studies.studies[0].study.validation_seen=true;
    let mut world=world(studies);
    world.init_resource::<crate::document::DocumentRegistry>();
    world.init_resource::<Assets<Image>>();world.init_resource::<Reconstructions>();
    world.init_resource::<bevy::ecs::message::Messages<FieldMsg>>();
    world.init_resource::<bevy::ecs::message::Messages<crate::app::actions::Act<StudyAction>>>();
    world.init_resource::<bevy::input_focus::InputFocus>();
    world.init_resource::<crate::app::actions::InFlight<StudyAction>>();world.init_resource::<crate::app::actions::Replies>();
    world.spawn((super::super::forms::FIELD,crate::ui_kit::text::TextField::new("Measured study fixture")));
    let mut schedule=Schedule::default();
    schedule.add_systems((super::super::forms::input,super::super::actions::apply,super::super::jobs::poll,chart::receive,rebuild_on_publication,collect).chain());
    schedule.run(&mut world);world.clear_trackers();
    let mut panels=world.query_filtered::<Entity,With<ReconstructedPanel>>();
    let original=panels.single(&world).unwrap();
    assert_eq!(world.resource::<Reconstructions>().0,1);
    world.write_message(crate::app::actions::Act::quiet(StudyAction::Status));schedule.run(&mut world);world.clear_trackers();
    let queried=panels.single(&world).unwrap();let count=world.resource::<Reconstructions>().0;
    for _ in 0..3 {world.write_message(crate::app::actions::Act::quiet(StudyAction::Status));schedule.run(&mut world);world.clear_trackers();}
    assert_eq!(panels.single(&world).unwrap(),queried,"already exposed full Status reads do not reconstruct controls");assert_eq!(world.resource::<Reconstructions>().0,count);
    for _ in 0..3{schedule.run(&mut world);world.clear_trackers();}
    assert_eq!(panels.single(&world).unwrap(),original,"idle queues/cache observation preserve actual controls");
    assert_eq!(world.resource::<Reconstructions>().0,1);
    let stamp=world.resource::<StudyOwner>().active().unwrap().stamp();
    world.resource_mut::<StudyUi>().focus=Some((Some(stamp),Field::Notes));
    schedule.run(&mut world);world.clear_trackers();
    let focused=panels.single(&world).unwrap();let count=world.resource::<Reconstructions>().0;
    world.write_message(FieldMsg{field:super::super::forms::FIELD,event:FieldEvent::Changed(TextDraft::new("real retained text",false))});
    schedule.run(&mut world);world.clear_trackers();
    assert_ne!(panels.single(&world).unwrap(),focused);assert_eq!(world.resource::<Reconstructions>().0,count+1);
    let text_panel=panels.single(&world).unwrap();let count=world.resource::<Reconstructions>().0;
    schedule.run(&mut world);world.clear_trackers();assert_eq!(panels.single(&world).unwrap(),text_panel);assert_eq!(world.resource::<Reconstructions>().0,count);
    // The real jobs terminal seam publishes a retained failure and increments
    // the owner before the same panel gate, rather than manually setting dirty.
    pending(world.resource_mut::<StudyOwner>().bypass_change_detection());
    world.clear_trackers();
    schedule.run(&mut world);world.clear_trackers();
    assert_ne!(panels.single(&world).unwrap(),text_panel);assert_eq!(world.resource::<Reconstructions>().0,count+1);
    let terminal_panel=panels.single(&world).unwrap();
    for _ in 0..3{schedule.run(&mut world);world.clear_trackers();}
    assert_eq!(panels.single(&world).unwrap(),terminal_panel);assert_eq!(world.resource::<Reconstructions>().0,count+1);
}
#[test]
fn reopened_review_renders_late_cancellation_request_separate_from_completed_execution() {
    let mut owner=owner();let captured=owner.studies[0].study.clone();let stamp=owner.studies[0].stamp();
    let trial=captured.archive.trials.iter().find(|t|t.comparison.passes).unwrap().clone();let ids=vec![trial.id.clone()];
    let mut completed=sim_runtime::experiment_study::evaluate(&captured.archive,&ids,&captured.baseline,&captured.draft,None,false,&AtomicBool::new(true),|_,_|{}).unwrap();
    completed.cancelled=false;completed.results[0].errors.clear();
    let prediction=sim_runtime::experiment_study::Prediction{trace:trial.predicted.clone(),metrics:trial.comparison.clone()};
    completed.results[0].baseline=Some(prediction.clone());completed.results[0].candidate=Some(prediction);
    owner.pending.push(PendingJob{id:7,kind:JobKind::Evaluate,stamp:Some(stamp),document:None,source:"fixture archive".into(),trial_ids:ids.clone(),cancel_requested:false,job:Job::<JobOutput>::finished(7,Ok(JobOutput::Evaluated(completed))),selection_epoch:0,gate:None,captured:Some(captured),launch:Value::Null});
    let registry=crate::document::DocumentRegistry::default();
    super::super::actions::apply_action(&mut owner,&registry,&StudyAction::Cancel{job:7}).unwrap();
    super::super::jobs::poll_owner(&mut owner,&registry);
    assert_eq!(owner.studies[0].study.evaluations[0].summary(&ids).passes,1);
    owner.studies[0].study.view.evaluation=Some(0);owner.studies[0].study.view.trial_id=Some(trial.id);
    // This is an in-memory saved-schema roundtrip, never a performed export.
    owner.studies[0].study=serde_json::from_value(serde_json::to_value(&owner.studies[0].study).unwrap()).unwrap();
    owner.studies[0].study.validate().unwrap();
    let mut world=world(owner);world.run_system_once(render).unwrap();
    let mut texts=world.query::<&Text>();let text=texts.iter(&world).map(|t|t.0.as_str()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("Cancellation requested: yes · execution cancelled: no"));
    assert!(text.contains("did not cancel the captured execution"));assert!(text.contains("Terminal job 7"));
}
#[test]
fn empty_study_owner_idle_frames_keep_actual_open_controls() {
    let mut world=world(StudyOwner::default());
    world.init_resource::<crate::document::DocumentRegistry>();world.init_resource::<Assets<Image>>();world.init_resource::<Reconstructions>();
    let mut schedule=Schedule::default();schedule.add_systems((super::super::jobs::poll,chart::receive,rebuild_on_publication,collect).chain());
    schedule.run(&mut world);world.clear_trackers();
    let mut panels=world.query_filtered::<Entity,With<ReconstructedPanel>>();let original=panels.single(&world).unwrap();
    assert!(world.resource::<StudyUi>().rendered.values().any(|c|matches!(c.hit,Hit::Path{field:Field::Archive,..})));
    for _ in 0..3{schedule.run(&mut world);world.clear_trackers();}
    assert_eq!(panels.single(&world).unwrap(),original);assert_eq!(world.resource::<Reconstructions>().0,1);
}

// T48 actual rendered-control fixtures. Written but unexecuted by batch contract.
#[test]
fn refinement_actual_inputs_actions_selections_and_busy_bindings() {
    let mut o=owner();
    o.studies[0].study.refinement.scenarios=sim_runtime::experiment_study::refinement::default_scenarios(&o.studies[0].study);
    let mut world=world(o);world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    let ui=world.resource::<StudyUi>();
    for path in ["experiment:/name","experiment:/device","experiment:/fixture","experiment:/component_id","experiment:/initial_encoder_rad","experiment:/timing/evidence","experiment:/timing/period_s","experiment:/controller/parameters/kp","experiment:/limits/rms_rad","experiment:/trajectory/0/time_s","experiment:/trajectory/0/position_rad","scenarios:/0/label","scenarios:/0/timing/command_delay_ticks"] {
        assert!(ui.rendered.values().any(|c|matches!(&c.hit,Hit::Focus{field:Field::Refinement(p),..} if p==path)),"actual Button input for {path}");
    }
    for id in ["simulate","sensitivity","fit","robustness"] {
        let Hit::Action(StudyAction::RefineRun{stamp,..})=super::super::forms::activate(ui,&format!("study:refine:run:{id}")).unwrap()else{panic!("wrong typed action")};
        assert_eq!(stamp,StudyStamp{id:11,revision:4});
    }
    assert!(ui.rendered.values().any(|c|matches!(&c.hit,Hit::RefineTrial{role,..}if role=="train")));
    let mut o=owner();pending(&mut o);let mut world=world(o);world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    for id in ["simulate","sensitivity","fit","robustness"]{assert!(!world.resource::<StudyUi>().rendered[&format!("study:refine:run:{id}")].enabled);}
}
#[test]
fn refinement_scalar_submission_preserves_electrical_and_stamped_late_drafts() {
    let mut o=owner();let stamp=o.active().unwrap().stamp();
    let action=super::super::forms::submission(&o,Some(stamp),&Field::Refinement("experiment:/timing/evidence".into()),"captured source link").unwrap();
    let StudyAction::RefineApply{command:sim_runtime::experiment_study::refinement::Command::SetExperiment(e),..}=action else{panic!("wrong shared mutation")};
    assert_eq!(e.timing.evidence,"captured source link");assert_eq!(e.electrical,o.active().unwrap().study.refinement.experiment.electrical);
    assert!(super::super::forms::submission(&o,Some(stamp),&Field::Refinement("experiment:/device".into()),"2.5").unwrap_err().contains("refinement.experiment:/device"));
    o.studies[0].revision+=1;let mut ui=StudyUi::default();
    assert!(super::super::forms::text_submission(&mut ui,&o,Hit::Focus{stamp:Some(stamp),field:Field::Refinement("experiment:/fixture".into()),text:String::new()},"late fixture").is_err());
    assert!(ui.drafts.values().any(|v|v=="late fixture"));assert!(ui.blocking_reason().is_some());
}
#[test]
fn actual_refinement_review_never_scores_cancelled_run_and_notes_are_distinct() {
    use sim_runtime::controller_refinement::control::{Run,TrackingScore};
    let mut o=owner();let s=&mut o.studies[0].study;
    s.refinement.controller_runs.push(Run{version:1,experiment:s.refinement.experiment.clone(),model:s.draft.clone(),runtime:sim_runtime::physics_context::RuntimeIdentity::current(),frames:vec![],truth:vec![[0.013,0.1],[0.097,0.2]],electrical:None,score:Some(TrackingScore{rms_rad:0.,peak_rad:0.,settled_error_rad:0.,saturation_fraction:0.,error_sign_changes:0,passes:true}),failure:None,cancelled:true,evidence_kind:"synthetic unexecuted fixture".into()});
    let mut world=world(o);world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    let ui=world.resource::<StudyUi>();
    let Hit::Action(StudyAction::RefineApply{command:sim_runtime::experiment_study::refinement::Command::SetDecision{kind,index,..},..})=super::super::forms::activate(ui,"study:refine:decision:controller:0:reviewed").unwrap()else{panic!("review action must remain separate from candidate use")};
    assert_eq!(kind,"controller");assert_eq!(index,0);
    assert!(!ui.rendered.contains_key("study:refine:use-fit:0"));
    let Hit::Action(StudyAction::RefineApply{command:sim_runtime::experiment_study::refinement::Command::SelectControllerRun(index),..})=super::super::forms::activate(ui,"study:refine:chart-run:0").unwrap()else{panic!("captured chart selector must use shared typed action")};
    assert_eq!(index,0);
    let mut texts=world.query::<&Text>();let text=texts.iter(&world).map(|t|t.0.as_str()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("UNSCORED incomplete/failed/cancelled"));assert!(text.contains("physical t=0.013000 s"));assert!(text.contains("physical t=0.097000 s"));
}

// T48 repair fixtures are source-reviewed only, never executed in this batch.
#[test]
fn rhai_parameter_rows_keep_schema_named_keys_and_nested_values_authorable() {
    use sim_runtime::{controller_refinement::control::Policy,experiment_study::refinement::Command};
    let mut o=owner();let stamp=o.active().unwrap().stamp();
    o.studies[0].study.refinement.experiment.controller=Policy::Rhai{source:"fn control(t,s,a,state) { #{commands: #{duty: 0.0}, state: state} }".into(),parameters:serde_json::json!({"kind":{"version":[1,2]},"power":true}),duty_limit:0.5};
    let added=super::super::forms::submission(&o,Some(stamp),&Field::Refinement("parameter_new:".into()),"electrical = {\"gains\":[1,2]}").unwrap();
    let StudyAction::RefineApply{command:Command::SetExperiment(e),..}=added else{panic!("parameter row must submit typed experiment")};
    let Policy::Rhai{parameters,..}=e.controller else{panic!("policy changed")};
    assert_eq!(parameters["electrical"]["gains"],serde_json::json!([1,2]));
    let mut world=world(o);world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    let ui=world.resource::<StudyUi>();
    for name in ["kind","power"] {
        assert!(ui.rendered.values().any(|c|matches!(&c.hit,Hit::Focus{field:Field::Refinement(path),..}if path==&format!("parameter_value:{name}"))));
        assert!(ui.rendered.contains_key(&format!("study:refine:parameter:remove:{name}")));
    }
}

#[test]
fn immutable_publication_projection_retains_raw_rejected_text_without_acknowledging_it() {
    let mut ui=StudyUi::default();
    ui.drafts.insert((Some((11,4)),Field::Refinement("experiment:/device".into())),"invalid device text".into());
    ui.drafts.insert((Some((22,1)),Field::Notes),"another study remains separate".into());
    ui.error=Some("current global parse diagnostic, not attributed to every draft".into());
    let projection=ui.publication_inputs(11);
    assert!(projection["current_global_diagnostic"].as_str().unwrap().contains("global parse diagnostic"));
    assert_eq!(projection["drafts"].as_array().unwrap().len(),1);
    assert_eq!(projection["drafts"][0]["text"],"invalid device text");
    assert!(ui.blocking_reason().is_some());assert_eq!(ui.drafts.len(),2);
}

#[test]
fn rendered_refinement_activation_converts_original_stamped_hit() {
    use crate::ui_kit::activation::{Activated,Ordinary};
    let mut world=world(owner());
    world.init_resource::<bevy::input_focus::InputFocus>();
    world.init_resource::<Messages<crate::ui_kit::text::FieldMsg>>();
    world.init_resource::<Messages<crate::app::actions::Act<StudyAction>>>();
    world.run_system_once(render).unwrap();
    let entity=world.query::<(Entity,&ControlId)>().iter(&world)
        .find_map(|(e,id)|(id.0=="study:refine:run:simulate").then_some(e)).unwrap();
    assert!(world.get::<Ordinary>(entity).is_some());
    assert!(world.get::<bevy::ui_widgets::ActivateOnPress>(entity).is_some());
    world.entity_mut(entity).insert(Activated);
    world.run_system_once(super::super::forms::input).unwrap();
    let actions:Vec<_>=world.resource_mut::<Messages<crate::app::actions::Act<StudyAction>>>().drain().collect();
    assert_eq!(actions.len(),1);
    let StudyAction::RefineRun{stamp,..}=&actions[0].action else{panic!("wrong action")};
    assert_eq!(*stamp,StudyStamp{id:11,revision:4});
}

#[test]
fn rendered_old_study_field_cannot_take_focus_for_replaced_revision() {
    use crate::ui_kit::activation::Activated;
    let mut world=world(owner());
    world.init_resource::<bevy::input_focus::InputFocus>();
    world.init_resource::<Messages<crate::ui_kit::text::FieldMsg>>();
    world.init_resource::<Messages<crate::app::actions::Act<StudyAction>>>();
    world.run_system_once(render).unwrap();
    let entity=world.query::<(Entity,&Hit)>().iter(&world).find_map(|(e,h)|
        matches!(h,Hit::Focus{field:Field::Refinement(path),..} if path=="experiment:/name").then_some(e)).unwrap();
    world.resource_mut::<StudyOwner>().studies[0].revision+=1;
    world.entity_mut(entity).insert(Activated);
    world.run_system_once(super::super::forms::input).unwrap();
    assert!(world.resource::<StudyUi>().focus.is_none());
    assert!(world.resource::<StudyUi>().error.as_ref().unwrap().contains("revision changed"));
}

#[test]
fn actual_study_field_anchor_identity_ignores_revision_and_displayed_text() {
    let mut world=world(owner());
    world.run_system_once(render).unwrap();
    let identity=|world:&mut World| {
        world.query::<(&Hit,&crate::ui_kit::activation::InputIdentity)>().iter(world)
            .filter_map(|(hit,identity)|matches!(hit,Hit::Focus{field:Field::Refinement(path),..}
                if path=="experiment:/name").then_some(identity.0.clone())).collect::<Vec<_>>()
    };
    let old=identity(&mut world);
    assert_eq!(old.len(),1);
    world.resource_mut::<StudyOwner>().studies[0].revision+=1;
    world.resource_mut::<StudyOwner>().studies[0].study.refinement.experiment.name="Changed display".into();
    world.run_system_once(render).unwrap();
    let both=identity(&mut world);
    assert_eq!(both.len(),2);
    assert!(both.iter().all(|value|value==&old[0]),"focus anchor survives semantic revision/text publication");
}

fn recording_owner()->StudyOwner {
    let mut o=owner();let s=&mut o.studies[0].study;
    let r=sim_runtime::controller_refinement::recording::Recording{version:1,experiment:s.refinement.experiment.clone(),runtime:sim_runtime::physics_context::RuntimeIdentity::current(),frames:vec![],stop_request_s:0.,stop_receipt_s:0.,completed:false,failure:Some("unexecuted incomplete capture fixture".into()),stop_verified:false,initial_registers:Value::Null,transactions_origin_host_s:0.,timing_evidence:"host-clock seconds; no samples captured".into(),source_hashes:BTreeMap::from([("fixture".into(),"a".repeat(64))])};
    let hash=r.fingerprint();s.refinement_evidence.selected_recording=Some(hash.clone());s.refinement_evidence.recording_identities.push(hash);s.refinement.recordings.push(r);o
}
#[test]
fn actual_recording_controls_keep_incomplete_capture_inspectable_and_unscored(){
    let mut world=world(recording_owner());world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    let ui=world.resource::<StudyUi>();assert!(!ui.rendered["study:recording:predict"].enabled);
    assert!(ui.rendered.keys().any(|id|id.starts_with("study:recording:select:")));
    assert!(ui.rendered.values().any(|c|matches!(&c.hit,Hit::Focus{field:Field::Recording(p),..} if p.ends_with("/coordinate_frame"))));
    assert!(ui.rendered.values().any(|c|matches!(&c.hit,Hit::Focus{field:Field::Recording(p),..} if p.ends_with("/uncertainty_bounds"))));
    assert!(ui.rendered.values().filter(|c|matches!(c.hit,Hit::FreezeRecording{..})).all(|c|!c.enabled));
}
#[test]
fn actual_purpose_activation_uses_one_stamped_owner_and_original_identity(){
    use sim_runtime::controller_refinement::recording::Purpose;
    let mut world=world(recording_owner());world.init_resource::<bevy::input_focus::InputFocus>();world.init_resource::<Messages<crate::ui_kit::text::FieldMsg>>();world.init_resource::<Messages<crate::app::actions::Act<StudyAction>>>();
    world.run_system_once(render).unwrap();
    let e=world.query::<(Entity,&ControlId)>().iter(&world).find_map(|(e,id)|(id.0=="study:recording:purpose:ClosedLoopPrediction").then_some(e)).unwrap();
    assert!(world.get::<crate::ui_kit::activation::Ordinary>(e).is_some());world.entity_mut(e).insert(crate::ui_kit::activation::Activated);
    world.run_system_once(super::super::forms::input).unwrap();
    let messages:Vec<_>=world.resource_mut::<Messages<crate::app::actions::Act<StudyAction>>>().drain().collect();
    assert!(matches!(&messages[0].action,StudyAction::RefineApply{stamp:StudyStamp{id:11,revision:4},command:sim_runtime::experiment_study::refinement::Command::SetPredictionPurpose(Purpose::ClosedLoopPrediction)}));
}
#[test]
fn actual_setup_drafts_survive_modal_suspension_and_reject_stale_submit(){
    let mut world=world(recording_owner());world.init_resource::<bevy::input_focus::InputFocus>();world.init_resource::<Messages<crate::ui_kit::text::FieldMsg>>();world.init_resource::<Messages<crate::app::actions::Act<StudyAction>>>();
    world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    let hit=world.resource::<StudyUi>().rendered.values().find_map(|c|matches!(&c.hit,Hit::Focus{field:Field::Recording(p),..} if p.ends_with("/fixture")).then_some(c.hit.clone())).unwrap();
    let Hit::Focus{stamp,field,..}=hit.clone()else{panic!("field")};
    world.resource_mut::<StudyUi>().focus=Some((stamp,field.clone()));world.resource_mut::<StudyUi>().buffer="retained fixture draft".into();
    world.resource_mut::<StudyUi>().drafts.insert((stamp.map(|s|(s.id,s.revision)),field.clone()),"retained fixture draft".into());
    let mut editor=crate::ui_kit::text::TextField::new("fixture");editor.suspended=true;
    let e=world.spawn((super::super::forms::FIELD,editor)).id();
    world.resource_mut::<Messages<crate::ui_kit::text::FieldMsg>>().write(crate::ui_kit::text::FieldMsg{field:super::super::forms::FIELD,event:crate::ui_kit::text::FieldEvent::Submit("must not apply under modal".into())});
    world.run_system_once(super::super::forms::input).unwrap();assert_eq!(world.resource::<StudyUi>().buffer,"retained fixture draft");
    world.get_mut::<crate::ui_kit::text::TextField>(e).unwrap().suspended=false;world.resource_mut::<StudyOwner>().studies[0].revision+=1;
    world.resource_mut::<Messages<crate::ui_kit::text::FieldMsg>>().write(crate::ui_kit::text::FieldMsg{field:super::super::forms::FIELD,event:crate::ui_kit::text::FieldEvent::Submit("retained fixture draft".into())});
    world.run_system_once(super::super::forms::input).unwrap();assert!(world.resource::<StudyUi>().error.as_ref().unwrap().contains("replaced or edited"));
    assert!(world.resource::<StudyUi>().publication_inputs(11)["drafts"].as_array().unwrap().iter().any(|d|d["text"]=="retained fixture draft"));
    assert_eq!(world.resource_mut::<Messages<crate::app::actions::Act<StudyAction>>>().drain().count(),0);
}
#[test]
fn actual_recording_import_and_additional_fields_capture_paths_without_frame_io(){
    let mut world=world(recording_owner());world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    for (name,text) in [("import","/missing/controller-capture.json"),("additional","/missing/saved-study.json")]{
        let hit=world.resource::<StudyUi>().rendered.values().find_map(|c|matches!(&c.hit,Hit::Focus{field:Field::Recording(p),..} if p==name).then_some(c.hit.clone())).unwrap();
        let owner=world.resource::<StudyOwner>();
        let action=super::super::forms::submission(owner,Some(StudyStamp{id:11,revision:4}),&Field::Recording(name.into()),text).unwrap();
        match action{StudyAction::ImportRecording{stamp,path}=>{assert_eq!(stamp.revision,4);assert_eq!(path,text)},StudyAction::FitCombined{stamp,additional_path}=>{assert_eq!(stamp.id,11);assert_eq!(additional_path.as_deref(),Some(text))},_=>panic!("wrong path action")}
        assert!(matches!(hit,Hit::Focus{..}));
    }
    assert!(world.resource::<StudyOwner>().pending.is_empty(),"rendering/parsing paths does not read files or execute runtime");
}
#[test]
fn structured_unknown_value_entry_appends_estimate_without_rewriting_capture(){
    let o=recording_owner();let retained=o.active().unwrap();let hash=retained.study.refinement.recordings[0].fingerprint();
    let before=serde_json::to_value(&retained.study.refinement.recordings[0]).unwrap();
    let action=super::super::forms::recording_forms::submission(&retained.study,retained.stamp(),&format!("{hash}:/properties/0/value"),"0.25").unwrap();
    let StudyAction::RefineApply{command:sim_runtime::experiment_study::refinement::Command::AppendContext{context},..}=action else{panic!("revision command")};
    assert_eq!(context.properties[0].value,Some(0.25));assert_eq!(context.properties[0].origin,sim_runtime::controller_refinement::context::Origin::Estimated);
    context.validate().unwrap();assert_eq!(serde_json::to_value(&retained.study.refinement.recordings[0]).unwrap(),before);
}
#[test]
fn rendered_recording_editor_real_modal_containment_restores_owner_before_stale_refusal(){
    use crate::ui_kit::{activation::{self,Activated,ModalFocus,ModalPriority},text::{TextFieldApp,TextField,FieldMsg,FieldEvent}};
    use bevy::input_focus::{InputFocus,FocusCause};
    for electrical in [false,true] {
    let mut app=App::new();app.insert_resource(if electrical {electrical_ui_tests::electrical_owner()}else{recording_owner()}).insert_resource(StudyUi::default()).insert_resource(fonts());
    app.add_plugins(crate::ui_kit::text::TextEntryPlugin).add_text_field(super::super::forms::FIELD,TextField::new("Recording editor"));
    app.add_message::<crate::app::actions::Act<StudyAction>>().add_systems(Startup,render).add_systems(Update,super::super::forms::input.in_set(crate::app::InputSet::Window));
    crate::app::configure_sets(&mut app);activation::install(&mut app);app.update();
    let anchor=app.world_mut().query::<(Entity,&Hit)>().iter(app.world()).find_map(|(e,h)|(if electrical {matches!(h,Hit::Focus{field:Field::Electrical(p),..}if p=="source:/evidence")}else{matches!(h,Hit::Focus{field:Field::Recording(p),..}if p.ends_with("/fixture"))}).then_some(e)).unwrap();
    app.world_mut().resource_mut::<InputFocus>().set(anchor,FocusCause::Navigated);app.world_mut().entity_mut(anchor).insert(Activated);app.update();
    let editor=app.world_mut().query::<(Entity,&crate::ui_kit::text::FieldId)>().iter(app.world()).find_map(|(e,id)|(*id==super::super::forms::FIELD).then_some(e)).unwrap();
    let mapping=app.world().resource::<StudyUi>().focus.clone();assert!(mapping.is_some());
    let fixture_fonts=fonts();let k=Kit::new(&fixture_fonts);
    let modal=app.world_mut().spawn((Node::default(),ModalFocus,ModalPriority(100),bevy::ui::prelude::AccessibleLabel("Pending close fixture".into()))).id();
    app.world_mut().spawn((k.button("Cancel close",Hit::Action(StudyAction::Status),Look::Secondary,true),ChildOf(modal)));app.update();
    assert!(app.world().get::<TextField>(editor).unwrap().suspended);assert_eq!(app.world().resource::<StudyUi>().focus,mapping);
    app.world_mut().despawn(modal);app.update();assert!(!app.world().get::<TextField>(editor).unwrap().suspended);assert_eq!(app.world().resource::<InputFocus>().get(),Some(editor));assert_eq!(app.world().resource::<StudyUi>().focus,mapping);
    app.world_mut().resource_mut::<StudyOwner>().studies[0].revision+=1;
    app.world_mut().write_message(FieldMsg{field:super::super::forms::FIELD,event:FieldEvent::Submit("restored but now stale setup draft".into())});app.update();
    assert!(app.world().resource::<StudyUi>().error.as_ref().unwrap().contains("replaced or edited"));assert!(app.world().resource::<StudyUi>().publication_inputs(11)["drafts"].as_array().unwrap().iter().any(|d|d["text"]=="restored but now stale setup draft"));
    }
}
#[test]
fn queued_recording_authoring_does_not_acknowledge_input_before_terminal_validation(){
    let o=recording_owner();let stamp=o.active().unwrap().stamp();let mut ui=StudyUi::default();
    let hit=Hit::Focus{stamp:Some(stamp),field:Field::Recording("import".into()),text:String::new()};
    let action=super::super::forms::text_submission(&mut ui,&o,hit,"/missing/rejected-capture.json").unwrap();
    super::super::forms::acknowledge(&mut ui,&action,&sim_api::Outcome::Done(Ok(json!({"job":42}))));
    assert_eq!(ui.drafts.len(),1);assert_eq!(ui.awaiting.len(),1);assert!(ui.pending_submission_jobs.contains_key(&42));assert!(ui.publication_inputs(stamp.id)["pending_submission_jobs"].as_array().unwrap().contains(&json!(42)));
}
#[test]
fn terminal_recording_validation_keeps_rejected_or_newer_text_and_only_clears_applied_input(){
    for (applied,newer) in [(false,false),(true,true),(true,false)]{
        let mut o=recording_owner();let stamp=o.active().unwrap().stamp();let mut ui=StudyUi::default();
        let hit=Hit::Focus{stamp:Some(stamp),field:Field::Recording("import".into()),text:String::new()};
        let action=super::super::forms::text_submission(&mut ui,&o,hit,"/missing/capture.json").unwrap();
        super::super::forms::acknowledge(&mut ui,&action,&sim_api::Outcome::Done(Ok(json!({"job":42}))));
        if newer{ui.drafts.insert((Some((stamp.id,stamp.revision)),Field::Recording("import".into())),"/newer/draft.json".into());}
        o.receipts.push(super::super::jobs::JobReceipt{id:42,kind:JobKind::RecordingImport,stamp:Some(stamp),source:"fixture".into(),trial_ids:vec![],launch:json!({"recording_applied":applied}),document:None,cancelled:false,displaced:false,stale:false,message:"unexecuted terminal fixture".into(),error:(!applied).then(||"recording.classification: rejected".into()),captured:None,evaluation:None,refinement:None});
        let mut world=world(o);world.insert_resource(ui);world.run_system_once(super::super::forms::terminal_submissions).unwrap();
        let ui=world.resource::<StudyUi>();assert_eq!(ui.drafts.is_empty(),applied&&!newer);assert!(ui.awaiting.is_empty());assert!(ui.pending_submission_jobs.is_empty());
        if !applied{assert!(ui.error.as_ref().unwrap().contains("classification"));}
        if newer{assert!(ui.drafts.values().any(|v|v=="/newer/draft.json"));}
    }
}
#[test]
fn immediate_recording_group_refusal_retains_raw_fields_and_unlocks_retry(){
    let mut world=world(recording_owner());world.run_system_once(render).unwrap();world.run_system_once(collect).unwrap();
    let hit=world.resource::<StudyUi>().rendered.values().find_map(|c|matches!(&c.hit,Hit::AddRecordingRow{kind,..}if kind=="artifact").then_some(c.hit.clone())).unwrap();
    let Hit::AddRecordingRow{stamp,hash,kind}=hit else{panic!("actual add artifact control")};
    let prefix=format!("{hash}:new/{kind}/");let fields=[("role","capture setup"),("location","fixture/artifact.step"),("blake3","aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")];
    for(name,text)in fields{world.resource_mut::<StudyUi>().drafts.insert((Some((stamp.id,stamp.revision)),Field::Recording(format!("{prefix}{name}"))),text.into());}
    let action=super::super::forms::recording_forms::add_row(&world.resource::<StudyOwner>().active().unwrap().study,world.resource::<StudyUi>(),stamp,&hash,&kind).unwrap();
    super::super::forms::stage_group(&mut world.resource_mut::<StudyUi>(),stamp,&prefix,action.clone()).unwrap();
    for(name,_)in fields{assert!(world.resource::<StudyUi>().awaiting_key(&(Some((stamp.id,stamp.revision)),Field::Recording(format!("{prefix}{name}")))));}
    super::super::forms::acknowledge(&mut world.resource_mut::<StudyUi>(),&action,&sim_api::Outcome::Done(Err("study.recording: busy or stale original identity".into())));
    assert!(world.resource::<StudyUi>().awaiting.is_empty());assert!(world.resource::<StudyUi>().pending_submission_jobs.is_empty());assert_eq!(world.resource::<StudyUi>().drafts.len(),3);
    for(name,_)in fields{assert!(!world.resource::<StudyUi>().awaiting_key(&(Some((stamp.id,stamp.revision)),Field::Recording(format!("{prefix}{name}")))));}
    assert!(super::super::forms::stage_group(&mut world.resource_mut::<StudyUi>(),stamp,&prefix,action).is_ok());
}

/// Captured attempts may have no usable outcome. Real controls still reach the
/// immutable dataset/chart consumer; review does not execute a new prediction.
#[test]
fn actual_fit_case_controls_review_additional_and_partial_sources_without_adoption(){
    use sim_runtime::{controller_refinement::{calibration as cal,calibration_data as data,control::{ControllerSession,Feedback},recording::{Recording,MeasuredFrame}},experiment_study::refinement::{self as shared,recordings}};
    let mut o=owner();let s=&mut o.studies[0].study;
    let e=s.refinement.experiment.clone();let mut session=ControllerSession::new(e.clone()).unwrap();
    let f=session.tick(0.015,Feedback{electrical:None,observed_s:0.01,request_s:0.009,completion_s:0.012,received_s:0.012,encoder_rad:0.1}).unwrap();
    let drive_counts=(f.applied_duty*1000.).round() as i16;
    let r=Recording{version:1,experiment:e,runtime:sim_runtime::physics_context::RuntimeIdentity::current(),frames:vec![MeasuredFrame{control:f,command_request_s:0.016,command_receipt_s:0.02,drive_counts,voltage_v:12.,temperature_c:25.,current_raw_uncalibrated:0}],stop_request_s:0.03,stop_receipt_s:0.04,completed:true,failure:None,stop_verified:true,initial_registers:Value::Null,transactions_origin_host_s:0.,timing_evidence:"Synthetic original sample times".into(),source_hashes:BTreeMap::from([("synthetic acquisition".into(),"original captured run".into())])};
    let hash=r.fingerprint();let assignment=data::Assignment{recording_hash:hash.clone(),role:data::Role::HeldOut,limits:sim_runtime::experiment_comparison::Limits{rmse:0.01,final_abs_error:0.02},rationale:"Reserved immutable case".into()};
    let mut tuning=r.clone();tuning.source_hashes.insert("synthetic acquisition".into(),"distinct run".into());
    let train=data::Assignment{recording_hash:tuning.fingerprint(),role:data::Role::Train,limits:assignment.limits.clone(),rationale:"Reserved tuning run".into()};
    for recording in [r.clone(),tuning.clone()]{shared::apply(s,shared::Command::ImportRecording{recording}).unwrap();}
    for assignment in [assignment.clone(),train.clone()]{shared::apply(s,shared::Command::AssignRecording{assignment}).unwrap();}
    let dataset=data::RecordingDataset::capture(&[r,tuning],&[assignment,train]).unwrap();
    let initial=s.draft.conditions.load_inertia;
    let coordinates=vec![cal::Coordinate{device:None,path:"condition.load_inertia".into(),lower:initial*0.5,upper:initial*1.5}];
    let make_attempt=|cases:Vec<data::Case>,fingerprint:String|{
        let request=cal::FitRequest{model:cal::Family{shared:s.draft.clone(),device_deltas:BTreeMap::new()},training_ids:cases.iter().filter(|c|c.split=="train").map(|c|c.id.clone()).collect(),validation_ids:cases.iter().filter(|c|c.split!="train").map(|c|c.id.clone()).collect(),coordinates:coordinates.clone(),maximum_evaluations:40,validation_influenced:false};
        let scores=cases.into_iter().map(|c|{
            let metrics=sim_runtime::experiment_comparison::compare(&c.measured,&c.measured,&c.limits).unwrap();
            let missing=c.id==hash;
            cal::TrialScore{id:c.id,device:c.device,split:c.split,baseline:Some(metrics.clone()),candidate:(!missing).then_some(metrics),failure:missing.then(||"Candidate comparison not reached".into()),baseline_prediction:Some(c.measured.clone()),candidate_prediction:(!missing).then_some(c.measured)}
        }).collect();
        let partial=cal::Fit{baseline:request.model.clone(),candidate:request.model.clone(),coordinates:request.coordinates.clone(),training_ids:request.training_ids.clone(),validation_ids:request.validation_ids.clone(),optimizer:json!({"values":[initial]}),scores,validation_influenced:false,runtime:sim_runtime::physics_context::RuntimeIdentity::current(),status:"partial / unscored".into()};
        cal::FitAttempt{request,archive_hash:fingerprint,runtime:sim_runtime::physics_context::RuntimeIdentity::current(),evaluations:vec![],outcome:None,partial:Some(partial),failure:Some("Scoring interrupted before final comparison".into()),cancelled:true}
    };
    use sim_runtime::controller_refinement::calibration_data::CalibrationData;
    let recording_attempt=make_attempt(dataset.cases().unwrap(),dataset.fingerprint());
    let mut extra=s.archive.clone();for trial in &mut extra.trials{trial.id=format!("saved-additional:{}",trial.id);}
    let additional_id=extra.trials[0].id.clone();let additional_samples=extra.trials[0].measured.samples.len();let original_span=(extra.trials[0].measured.samples.first().unwrap().time_s,extra.trials[0].measured.samples.last().unwrap().time_s);
    let combined=data::CombinedDataset{archives:vec![s.archive.clone(),extra],recordings:Some(dataset.clone())};
    let combined_attempt=make_attempt(combined.cases().unwrap(),combined.fingerprint());
    s.refinement.recording_fits.push(data::RecordingFitAttempt{dataset,attempt:recording_attempt});
    s.refinement.combined_fits.push(data::CombinedFitAttempt{dataset:combined,attempt:combined_attempt});
    recordings::cache_identities(s);
    let mut w=world(o);w.init_resource::<bevy::input_focus::InputFocus>();w.init_resource::<Messages<crate::ui_kit::text::FieldMsg>>();w.init_resource::<Messages<crate::app::actions::Act<StudyAction>>>();
    for (kind,case) in [("recording_fit",hash.as_str()),("combined_fit",additional_id.as_str()),("combined_fit",hash.as_str())]{
        w.run_system_once(render).unwrap();w.run_system_once(collect).unwrap();
        let id=format!("study:recording:case:{kind}:0:{case}");
        let stamp=w.resource::<StudyOwner>().active().unwrap().stamp();
        let entity=w.query::<(Entity,&ControlId,&Hit)>().iter(&w).find_map(|(e,c,h)|(c.0==id&&matches!(h,Hit::Action(StudyAction::RefineApply{stamp:s,..}) if *s==stamp)).then_some(e)).unwrap();
        assert!(w.get::<crate::ui_kit::activation::Ordinary>(entity).is_some());w.entity_mut(entity).insert(crate::ui_kit::activation::Activated);
        w.run_system_once(super::super::forms::input).unwrap();
        w.entity_mut(entity).remove::<crate::ui_kit::activation::Activated>();
        let actions:Vec<_>=w.resource_mut::<Messages<crate::app::actions::Act<StudyAction>>>().drain().collect();
        let action=actions[0].action.clone();
        let StudyAction::RefineApply{command:shared::Command::SelectFitCase{selection},..}=action.clone()else{panic!("actual fit case action")};
        let registry=crate::document::DocumentRegistry::default();
        super::super::actions::apply_action(&mut w.resource_mut::<StudyOwner>(),&registry,&action).unwrap();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);
        while !w.resource::<StudyOwner>().pending.is_empty(){
            super::super::jobs::poll_owner(&mut w.resource_mut::<StudyOwner>(),&registry);
            assert!(std::time::Instant::now()<deadline,"actual authoring job did not finish");std::thread::yield_now();
        }
        assert_eq!(w.resource::<StudyOwner>().active().unwrap().study.refinement_evidence.selected_fit_case,selection);
        // The exact rendered occurrence is stale after terminal attachment advances
        // its source revision; it cannot submit another job or retarget the chart.
        assert!(super::super::actions::apply_action(&mut w.resource_mut::<StudyOwner>(),&registry,&action).unwrap_err().contains("revision"));
        assert!(w.resource::<StudyOwner>().pending.is_empty());
        w.init_resource::<Assets<Image>>();w.run_system_once(super::recording_chart::request).unwrap();
        while !w.resource::<StudyUi>().recording_charts.current(w.resource::<StudyOwner>()){
            w.run_system_once(super::recording_chart::receive).unwrap();
            assert!(std::time::Instant::now()<deadline,"actual raster job did not finish");std::thread::yield_now();
        }
        assert!(w.resource::<StudyUi>().recording_charts.image.is_some());
        let (label,axes)=super::recording_chart::inspect_fixture(w.resource::<StudyOwner>()).unwrap();
        assert!(label.contains("UNSCORED"));assert!(label.contains(case));
        if case==hash {assert!(label.contains("baseline 1 / candidate 0"));}
        if case==additional_id {assert_eq!(axes.1,original_span);assert!(label.contains(&format!("baseline {additional_samples} / candidate {additional_samples}")));}
        let reopened:sim_runtime::experiment_study::Study=serde_json::from_value(serde_json::to_value(&w.resource::<StudyOwner>().studies[0].study).unwrap()).unwrap();
        reopened.validate().unwrap();assert_eq!(reopened.refinement_evidence.selected_fit_case,selection);
    }
    {let s=&mut w.resource_mut::<StudyOwner>().studies[0].study;s.refinement.combined_fits[0].attempt.failure=Some("replaced immutable source".into());recordings::cache_identities(s);}
    assert!(super::recording_chart::inspect_fixture(w.resource::<StudyOwner>()).unwrap_err().contains("stale"));
}

#[path="electrical_ui_tests.rs"]
mod electrical_ui_tests;

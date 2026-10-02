//! Immutable controller traces rasterized through the shared jobs abstraction.
use super::super::{StudyOwner,forms::StudyUi};
use bevy::prelude::*;
use crate::jobs::{Latest,Pool};
#[derive(Clone,Debug,PartialEq,Eq)]
struct Key{study:u64,runs:usize,experiment:String,model:String}
struct Raster{key:Key,angle:Vec<u8>,duty:Vec<u8>,angle_axes:((f64,f64),(f64,f64)),duty_axes:((f64,f64),(f64,f64))}
#[derive(Default)]
pub(crate) struct Charts{job:Latest<Raster>,requested:Option<Key>,drawn:Option<Key>,pub(crate) angle:Option<Handle<Image>>,pub(crate) duty:Option<Handle<Image>>,pub(crate) angle_axes:((f64,f64),(f64,f64)),pub(crate) duty_axes:((f64,f64),(f64,f64)),pub(crate) error:Option<String>}
fn selected(owner:&StudyOwner)->Option<(usize,&sim_runtime::controller_refinement::control::Run)> {
    let r=owner.active()?;let runs=&r.study.refinement.controller_runs;
    let index=r.study.refinement_evidence.controller_run.unwrap_or_else(||runs.len().saturating_sub(1));
    Some((index,runs.get(index)?))
}
fn key(owner:&StudyOwner)->Option<Key>{let r=owner.active()?;let(index,run)=selected(owner)?;Some(Key{study:r.id,runs:index,experiment:run.experiment.fingerprint(),model:run.model.fingerprint()})}
impl Charts{pub(crate) fn current(&self,owner:&StudyOwner)->bool{self.drawn.is_some()&&self.drawn==key(owner)}}
pub(crate) fn request(owner:Res<StudyOwner>,mut ui:ResMut<StudyUi>){
    let wanted=key(&owner);if ui.refinement_charts.requested==wanted{return}
    let chart=&mut ui.refinement_charts;chart.job.cancel();chart.requested=wanted.clone();chart.drawn=None;chart.error=None;
    let Some(key)=wanted else{return};let Some(run)=selected(&owner).map(|(_,run)|run)else{return};
    let target=run.frames.iter().map(|f|[f.time_s,f.target_rad]).collect::<Vec<_>>();
    let feedback=run.frames.iter().map(|f|[f.time_s,f.estimated_position_rad]).collect::<Vec<_>>();
    let duty=run.frames.iter().map(|f|[f.time_s,f.applied_duty]).collect::<Vec<_>>();let truth=run.truth.clone();
    chart.job.start(Pool::Compute,"Captured controller trace raster",move |_|{
        let(angle,y,x)=crate::chart::rasterize_span(&[(target.as_slice(),crate::chart::COLORS[0]),(feedback.as_slice(),crate::chart::COLORS[1]),(truth.as_slice(),crate::chart::COLORS[2])],None);
        let(duty,dy,dx)=crate::chart::rasterize_span(&[(duty.as_slice(),crate::chart::COLORS[3])],None);
        Ok(Raster{key,angle,duty,angle_axes:(y,x),duty_axes:(dy,dx)})
    });
}
pub(crate) fn receive(owner:Res<StudyOwner>,mut ui:ResMut<StudyUi>,mut images:ResMut<Assets<Image>>,mut builder:Option<ResMut<crate::builder::Builder>>){
    if ui.refinement_charts.job.pending().is_none(){return}
    let Some((_,result))=ui.bypass_change_detection().refinement_charts.job.poll()else{return};
    let chart=&mut ui.refinement_charts;
    match result{
        Ok(r)if Some(&r.key)==key(&owner).as_ref()=>{
            for(handle,pixels)in[(&mut chart.angle,r.angle),(&mut chart.duty,r.duty)]{let h=handle.get_or_insert_with(||images.add(crate::chart::blank_image())).clone();if let Some(mut image)=images.get_mut(&h){image.data=Some(pixels)}}
            chart.angle_axes=r.angle_axes;chart.duty_axes=r.duty_axes;chart.drawn=Some(r.key);chart.error=None;
        }
        Ok(_)=>return,
        Err(e)=>{if chart.requested!=key(&owner){return}chart.error=Some(e)}
    }
    ui.epoch+=1;if let Some(builder)=builder.as_mut(){builder.panel_dirty=true}
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_runtime::{experiment_study::{Study,refinement::{self,Command}},controller_refinement::control::Run,physics_context::RuntimeIdentity};
    // Unexecuted source fixture: archived chart keys bind to the chosen immutable run.
    #[test]
    fn saved_run_selection_chooses_archived_inputs_not_newest_or_current_draft() {
        let root=crate::workspace::root().unwrap();
        let archive=sim_runtime::experiment_comparison::hx_archive::load(&root.join(super::super::super::super::DEFAULT_ARCHIVE),root).unwrap();
        let mut study=Study::new(archive).unwrap();
        let run=Run{version:1,experiment:study.refinement.experiment.clone(),model:study.draft.clone(),runtime:RuntimeIdentity::current(),frames:vec![],truth:vec![[0.01,0.1]],electrical:None,score:None,failure:Some("synthetic incomplete fixture".into()),cancelled:false,evidence_kind:"unexecuted fixture".into()};
        let mut later=run.clone();later.experiment.name="Later captured run".into();
        study.refinement.controller_runs=vec![run.clone(),later];
        refinement::apply(&mut study,Command::SelectControllerRun(0)).unwrap();
        let reopened:Study=serde_json::from_value(serde_json::to_value(study).unwrap()).unwrap();
        let mut owner=StudyOwner::default();owner.retain(reopened,"unexecuted fixture".into(),None,false,true);
        let captured=key(&owner).unwrap();assert_eq!(captured.runs,0);assert_eq!(captured.experiment,run.experiment.fingerprint());
        owner.get_mut(1).unwrap().study.refinement.experiment.name="Current authoring edit".into();
        assert_eq!(key(&owner).unwrap(),captured,"draft edit cannot relabel captured chart");
        let before=serde_json::to_value(&owner.active().unwrap().study).unwrap();
        assert!(refinement::apply(&mut owner.get_mut(1).unwrap().study,Command::SelectControllerRun(99)).is_err());
        assert_eq!(before,serde_json::to_value(&owner.active().unwrap().study).unwrap());
    }
}

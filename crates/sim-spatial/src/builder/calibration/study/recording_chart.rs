//! Captured recording/prediction rasters use existing Latest jobs; images are
//! presentation caches. Study selection cannot retarget an in-flight raster.
use super::super::{StudyOwner,forms::StudyUi};
use bevy::prelude::*;
use crate::jobs::{Latest,Pool};
use sim_runtime::controller_refinement::recording::Purpose;
use sim_runtime::experiment_study::{Study, refinement::recordings::{self, FitCaseSelection}};
#[derive(Clone,Debug,PartialEq,Eq)]
enum Source{Recording{recording:String,purpose:Purpose,prediction:Option<usize>},Fit(FitCaseSelection)}
#[derive(Clone,Debug,PartialEq,Eq)]
struct Key{study:u64,source:Source}
struct Raster{key:Key,pixels:Vec<u8>,axes:((f64,f64),(f64,f64)),label:String}
#[derive(Default)]
pub(crate) struct Charts{job:Latest<Raster>,requested:Option<Key>,drawn:Option<Key>,pub(crate) image:Option<Handle<Image>>,pub(crate) axes:((f64,f64),(f64,f64)),pub(crate) error:Option<String>,pub(crate) label:String}
fn key(owner:&StudyOwner)->Option<Key>{
    let r=owner.active()?;
    if let Some(selection)=&r.study.refinement_evidence.selected_fit_case {
        // Derived identity was produced by the load/authoring job. A replaced
        // attempt cannot retarget a cached selection or raster.
        if recordings::fit_identity(&r.study,&selection.kind,selection.index)!=Some(selection.fit_blake3.as_str()){return None}
        return Some(Key{study:r.id,source:Source::Fit(selection.clone())});
    }
    let recording=r.study.refinement_evidence.selected_recording.clone()?;
    let purpose=r.study.refinement_evidence.prediction_purpose.unwrap_or(Purpose::RecordedCommandReplay);
    let prediction=r.study.refinement.predictions.iter().rposition(|p|p.recording_hash==recording&&p.purpose==purpose);
    Some(Key{study:r.id,source:Source::Recording{recording,purpose,prediction}})
}
/// Shared immutable resolution and rasterization, executed only by the existing
/// compute job. No candidate adoption, controller execution or new prediction.
fn fit_raster(study:&Study,key:Key,selection:&FitCaseSelection)->Result<Raster,String>{
    let traces=recordings::fit_case_traces(study,selection)?;
    let measured=traces.case.measured.samples.iter().map(|p|[p.time_s,p.value]).collect::<Vec<_>>();
    let baseline=traces.baseline.as_ref().map(|t|t.samples.iter().map(|p|[p.time_s,p.value]).collect::<Vec<_>>()).unwrap_or_default();
    let candidate=traces.candidate.as_ref().map(|t|t.samples.iter().map(|p|[p.time_s,p.value]).collect::<Vec<_>>()).unwrap_or_default();
    let label=format!("Immutable {} {} case {} · device {} · frozen role {} · measured {} / baseline {} / candidate {} samples · {} · failure {:?}",selection.kind,selection.index,selection.case_id,traces.case.device,traces.case.split,measured.len(),baseline.len(),candidate.len(),if traces.unscored||baseline.is_empty()||candidate.is_empty(){"UNSCORED: failed, partial or missing captured comparison traces"}else{"captured comparisons; candidate adoption is not required"},traces.failure);
    let(pixels,y,x)=crate::chart::rasterize_span(&[(&measured,crate::chart::COLORS[0]),(&baseline,crate::chart::COLORS[1]),(&candidate,crate::chart::COLORS[2])],None);
    Ok(Raster{key,pixels,axes:(y,x),label})
}
impl Charts{pub(crate) fn current(&self,owner:&StudyOwner)->bool{self.drawn.is_some()&&self.drawn==key(owner)}}
pub(crate) fn request(owner:Res<StudyOwner>,mut ui:ResMut<StudyUi>){
    let wanted=key(&owner);if ui.recording_charts.requested==wanted{return}
    let chart=&mut ui.recording_charts;chart.job.cancel();chart.requested=wanted.clone();chart.drawn=None;chart.error=None;
    let Some(key)=wanted else{return};let Some(r)=owner.active()else{return};
    if let Source::Fit(selection)=&key.source {
        let selection=selection.clone();let captured=r.study.clone();
        chart.job.start(Pool::Compute,"Immutable fit case sample-time raster",move |_|fit_raster(&captured,key,&selection));
        return;
    }
    let Source::Recording{recording,prediction,..}=&key.source else{return};
    let Some(captured)=recordings::cached_source(&r.study,recording).cloned()else{return};
    let prediction=prediction.and_then(|i|r.study.refinement.predictions.get(i)).cloned();
    chart.job.start(Pool::Compute,"Captured recording sample-time raster",move |_|{
    let (measured,predicted)=if let Some(p)=prediction{(p.measured.samples.clone(),p.predicted.samples.clone())}else{(captured.measured_trace().samples,vec![])};
    let measured=measured.into_iter().map(|o|[o.time_s,o.value]).collect::<Vec<_>>();
    let predicted=predicted.into_iter().map(|o|[o.time_s,o.value]).collect::<Vec<_>>();
    let(pixels,y,x)=crate::chart::rasterize_span(&[(measured.as_slice(),crate::chart::COLORS[0]),(predicted.as_slice(),crate::chart::COLORS[1])],None);Ok(Raster{key,pixels,axes:(y,x),label:"Captured recording · measured / predicted; absent prediction is UNSCORED".into()})});
}
pub(crate) fn receive(owner:Res<StudyOwner>,mut ui:ResMut<StudyUi>,mut images:ResMut<Assets<Image>>,mut builder:Option<ResMut<crate::builder::Builder>>){
    if ui.recording_charts.job.pending().is_none(){return}
    let Some((_,result))=ui.bypass_change_detection().recording_charts.job.poll()else{return};let chart=&mut ui.recording_charts;
    match result{Ok(r)if Some(&r.key)==key(&owner).as_ref()=>{let h=chart.image.get_or_insert_with(||images.add(crate::chart::blank_image())).clone();if let Some(mut image)=images.get_mut(&h){image.data=Some(r.pixels)}chart.axes=r.axes;chart.label=r.label;chart.drawn=Some(r.key);chart.error=None;},Ok(_)=>return,Err(e)=>{if chart.requested!=key(&owner){return}chart.error=Some(e)}}
    ui.epoch+=1;if let Some(builder)=builder.as_mut(){builder.panel_dirty=true}
}

#[cfg(test)]
pub(super) fn inspect_fixture(owner:&StudyOwner)->Result<(String,((f64,f64),(f64,f64))),String>{
    let key=key(owner).ok_or("stale or absent captured chart selection")?;
    let Source::Fit(selection)=key.source.clone()else{return Err("not a captured fit case".into())};
    let raster=fit_raster(&owner.active().unwrap().study,key,&selection)?;
    Ok((raster.label,raster.axes))
}

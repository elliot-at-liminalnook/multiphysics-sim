//! Captured recording/prediction rasters use existing Latest jobs; images are
//! presentation caches. Study selection cannot retarget an in-flight raster.
use super::super::{StudyOwner,forms::StudyUi};
use bevy::prelude::*;
use crate::jobs::{Latest,Pool};
use sim_runtime::controller_refinement::recording::Purpose;
#[derive(Clone,Debug,PartialEq,Eq)]
struct Key{study:u64,recording:String,purpose:Purpose,prediction:Option<usize>}
struct Raster{key:Key,pixels:Vec<u8>,axes:((f64,f64),(f64,f64))}
#[derive(Default)]
pub(crate) struct Charts{job:Latest<Raster>,requested:Option<Key>,drawn:Option<Key>,pub(crate) image:Option<Handle<Image>>,pub(crate) axes:((f64,f64),(f64,f64)),pub(crate) error:Option<String>}
fn key(owner:&StudyOwner)->Option<Key>{let r=owner.active()?;let recording=r.study.refinement_evidence.selected_recording.clone()?;let purpose=r.study.refinement_evidence.prediction_purpose.unwrap_or(Purpose::RecordedCommandReplay);let prediction=r.study.refinement.predictions.iter().rposition(|p|p.recording_hash==recording&&p.purpose==purpose);Some(Key{study:r.id,recording,purpose,prediction})}
impl Charts{pub(crate) fn current(&self,owner:&StudyOwner)->bool{self.drawn.is_some()&&self.drawn==key(owner)}}
pub(crate) fn request(owner:Res<StudyOwner>,mut ui:ResMut<StudyUi>){
    let wanted=key(&owner);if ui.recording_charts.requested==wanted{return}
    let chart=&mut ui.recording_charts;chart.job.cancel();chart.requested=wanted.clone();chart.drawn=None;chart.error=None;
    let Some(key)=wanted else{return};let Some(r)=owner.active()else{return};
    let Some(recording)=sim_runtime::experiment_study::refinement::recordings::cached_source(&r.study,&key.recording)else{return};
    let captured=recording.clone();
    let prediction=key.prediction.and_then(|i|r.study.refinement.predictions.get(i)).cloned();
    chart.job.start(Pool::Compute,"Captured recording sample-time raster",move |_|{
    let (measured,predicted)=if let Some(p)=prediction{(p.measured.samples.clone(),p.predicted.samples.clone())}else{(captured.measured_trace().samples,vec![])};
    let measured=measured.into_iter().map(|o|[o.time_s,o.value]).collect::<Vec<_>>();
    let predicted=predicted.into_iter().map(|o|[o.time_s,o.value]).collect::<Vec<_>>();
    let(pixels,y,x)=crate::chart::rasterize_span(&[(measured.as_slice(),crate::chart::COLORS[0]),(predicted.as_slice(),crate::chart::COLORS[1])],None);Ok(Raster{key,pixels,axes:(y,x)})});
}
pub(crate) fn receive(owner:Res<StudyOwner>,mut ui:ResMut<StudyUi>,mut images:ResMut<Assets<Image>>,mut builder:Option<ResMut<crate::builder::Builder>>){
    if ui.recording_charts.job.pending().is_none(){return}
    let Some((_,result))=ui.bypass_change_detection().recording_charts.job.poll()else{return};let chart=&mut ui.recording_charts;
    match result{Ok(r)if Some(&r.key)==key(&owner).as_ref()=>{let h=chart.image.get_or_insert_with(||images.add(crate::chart::blank_image())).clone();if let Some(mut image)=images.get_mut(&h){image.data=Some(r.pixels)}chart.axes=r.axes;chart.drawn=Some(r.key);chart.error=None;},Ok(_)=>return,Err(e)=>{if chart.requested!=key(&owner){return}chart.error=Some(e)}}
    ui.epoch+=1;if let Some(builder)=builder.as_mut(){builder.panel_dirty=true}
}

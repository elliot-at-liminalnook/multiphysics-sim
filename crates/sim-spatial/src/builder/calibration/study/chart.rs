//! Captured comparison presentation: only the shared raster draws lines.
//! The raster job owns immutable point arrays; no simulation occurs here.
use super::{StudyOwner, forms::StudyUi};
use bevy::prelude::*;
use crate::jobs::{Latest, Pool};
use sim_runtime::experiment_study::{Study, Evaluation};

pub(crate) const LABELS: [&str;4] = ["measured (hardware archive)", "predicted (fitted model, archive)", "captured baseline (shared runtime)", "captured candidate (shared runtime)"];
#[derive(Clone,Debug,PartialEq,Eq)]
struct Key { id:u64, trial:String, evaluation:Option<(usize,String)> }
struct Raster { key:Key, pixels:Vec<u8>, axes:((f64,f64),(f64,f64)) }
#[derive(Default)]
pub(crate) struct Chart {
    job:Latest<Raster>, requested:Option<Key>, drawn:Option<Key>,
    pub(crate) image:Option<Handle<Image>>,
    pub(crate) axes:((f64,f64),(f64,f64)),
    pub(crate) error:Option<String>,
}
impl Chart {
    pub(crate) fn current(&self, owner:&StudyOwner)->bool { self.drawn.is_some() && self.drawn==key(owner) }
}
pub(crate) fn evaluation(study:&Study)->Option<&Evaluation> { study.view.evaluation.and_then(|i|study.evaluations.get(i)) }
fn key(owner:&StudyOwner)->Option<Key> {
    let retained=owner.active()?;
    Some(Key {id:retained.id,trial:retained.study.view.trial_id.clone()?,evaluation:retained.study.view.evaluation.and_then(|i|retained.study.evaluations.get(i).map(|e|(i,e.id.clone())))})
}
/// SimSync starts jobs after all action/results publication; no frame raster work.
pub(crate) fn request(owner:Res<StudyOwner>,mut ui:ResMut<StudyUi>) {
    let wanted=key(&owner);
    if ui.chart.requested==wanted {return}
    ui.chart.job.cancel(); ui.chart.requested=wanted.clone(); ui.chart.error=None;
    let Some(wanted)=wanted else {ui.chart.drawn=None;return};
    let Some(retained)=owner.active() else {return};
    let s=&retained.study;
    let Some(trial)=s.archive.trials.iter().find(|t|t.id==wanted.trial) else {return};
    let pair=evaluation(s).and_then(|e|e.results.iter().find(|r|r.trial_id==trial.id));
    let points = |trace:&sim_runtime::experiment_comparison::Trace| trace.samples.iter().map(|o|[o.time_s,o.value]).collect::<Vec<_>>();
    let mut traces=vec![points(&trial.measured),points(&trial.predicted)];
    traces.push(pair.and_then(|r|r.baseline.as_ref()).map(|p|points(&p.trace)).unwrap_or_default());
    traces.push(pair.and_then(|r|r.candidate.as_ref()).map(|p|points(&p.trace)).unwrap_or_default());
    ui.chart.job.start(Pool::Compute,"Measured study comparison raster",move |_| {
        let borrowed:Vec<_>=traces.iter().enumerate().filter(|(_,p)|!p.is_empty()).map(|(i,p)|(p.as_slice(),crate::chart::COLORS[i])).collect();
        let (pixels,y,x)=crate::chart::rasterize_span(&borrowed,None);
        Ok(Raster{key:wanted,pixels,axes:(y,x)})
    });
}
/// JobResults never labels an old image as the current study/trial/evaluation.
pub(crate) fn receive(owner:Res<StudyOwner>,mut ui:ResMut<StudyUi>,mut images:ResMut<Assets<Image>>,mut builder:Option<ResMut<crate::builder::Builder>>) {
    if ui.chart.job.pending().is_none(){return}
    let Some((_,result))=ui.chart.job.poll() else {return};
    match result {
        Ok(raster) if Some(&raster.key)==key(&owner).as_ref()=>{
            let image=ui.chart.image.get_or_insert_with(||images.add(crate::chart::blank_image())).clone();
            if let Some(mut target)=images.get_mut(&image) {target.data=Some(raster.pixels);}
            ui.chart.axes=raster.axes; ui.chart.drawn=Some(raster.key); ui.chart.error=None;
        }
        Ok(_)=>{}, Err(e)=>ui.chart.error=Some(e),
    }
    ui.epoch+=1; if let Some(builder)=builder.as_mut() {builder.panel_dirty=true;}
}

//! Supervised teacher-to-student initialization. No simulation or robot logic.
use crate::neural::{Network,SupervisedSample};
use serde::{Deserialize,Serialize};
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Demonstration {pub observations:Vec<f64>,pub actions:Vec<f64>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dataset {pub version:u32,pub sensors:Vec<sim_core::Channel>,pub actuators:Vec<sim_core::Channel>,pub samples:Vec<Demonstration>}
impl Dataset {
    pub fn normalize(&self,network:&Network)->Result<Vec<SupervisedSample>,String>{
        if self.version!=1 || self.samples.is_empty(){return Err("distillation dataset requires version 1 and samples".into());}
        let bound=network.clone().bind(&self.sensors,&self.actuators)?;
        self.samples.iter().map(|s|bound.supervised_sample(&s.observations,&s.actions)).collect()
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FitConfig {pub epochs:usize,pub learning_rate:f64,pub batch_size:usize}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FitResult {pub policy:Network,pub initial_loss:f64,pub training_loss:Vec<f64>,pub best_epoch:Option<usize>,pub best_training_loss:f64}
pub fn fit(mut network:Network,samples:&[SupervisedSample],config:&FitConfig)->Result<FitResult,String>{
    if config.epochs==0 || config.epochs>100_000 || config.batch_size==0 || !config.learning_rate.is_finite() || config.learning_rate<=0.0 {return Err("invalid distillation optimizer settings".into());}
    let initial_loss=network.supervised_gradient(samples)?.0;
    let mut best=network.clone();let mut best_loss=initial_loss;let mut best_epoch=None;
    let mut optimizer=crate::optimization::Adam::new(network.parameters().len());
    let mut losses=vec![];
    for epoch in 0..config.epochs {
        // Deterministic cyclic batch order; no validation samples enter fitting.
        let chunks=samples.chunks(config.batch_size).collect::<Vec<_>>();
        for k in 0..chunks.len(){
            let batch=chunks[(k+epoch)%chunks.len()];
            let (_,gradient)=network.supervised_gradient(batch)?;
            let values=optimizer.step(&network.parameters(),&gradient,config.learning_rate,None)?;
            network=network.with_parameters(&values)?;
        }
        let loss=network.supervised_gradient(samples)?.0;losses.push(loss);
        if loss<best_loss {best_loss=loss;best=network.clone();best_epoch=Some(epoch);}
    }
    Ok(FitResult{policy:best,initial_loss,training_loss:losses,best_epoch,best_training_loss:best_loss})
}

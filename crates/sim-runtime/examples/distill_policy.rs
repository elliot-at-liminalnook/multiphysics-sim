//! Fit a portable policy from typed physical teacher demonstrations.
use serde::{Serialize,Deserialize};
use sim_domain_control::{neural::Network,distillation::{Dataset,FitConfig,fit}};
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Experiment{version:u32,network:Network,training:Dataset,validation:Dataset,optimizer:FitConfig}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    if args.len()!=2{return Err("usage: distill_policy experiment.json output-directory".into());}
    let bytes=std::fs::read(&args[0])?;let e:Experiment=serde_json::from_slice(&bytes)?;
    if e.version!=1{return Err("unsupported distillation experiment version".into());}
    let training=e.training.normalize(&e.network)?;let validation=e.validation.normalize(&e.network)?;
    std::fs::create_dir_all(&args[1])?;std::fs::write(format!("{}/experiment.json",args[1]),bytes)?;
    let initial_validation=e.network.supervised_gradient(&validation)?.0;
    let result=fit(e.network,&training,&e.optimizer)?;
    let final_validation=result.policy.supervised_gradient(&validation)?.0;
    let bound=result.policy.clone().bind(&e.validation.sensors,&e.validation.actuators)?;
    let mut maximum=vec![0.0f64;e.validation.actuators.len()];let mut squared=maximum.clone();
    for sample in &e.validation.samples{for (i,(a,b)) in bound.sample(&sample.observations)?.iter().zip(&sample.actions).enumerate(){maximum[i]=maximum[i].max((a-b).abs());squared[i]+=(a-b).powi(2);}}
    let physical_errors=e.validation.actuators.iter().enumerate().map(|(i,c)|serde_json::json!({"name":c.name,"unit":c.unit(),"maximum":maximum[i],"rms":(squared[i]/validation.len() as f64).sqrt()})).collect::<Vec<_>>();
    std::fs::write(format!("{}/policy.json",args[1]),serde_json::to_vec(&result.policy)?)?;
    std::fs::write(format!("{}/fit.json",args[1]),serde_json::to_vec(&result)?)?;
    let report=serde_json::json!({"version":1,"training_samples":training.len(),"validation_samples":validation.len(),"initial_training_loss":result.initial_loss,"final_training_loss":result.best_training_loss,"selected_epoch":result.best_epoch,"initial_validation_loss":initial_validation,"final_validation_loss":final_validation,"validation_physical_errors":physical_errors,"scope":"Checkpoint selected by training loss only. Supervised teacher-action imitation on held-out teacher states; not closed-loop stability, walking acceptance, sensor calibration or hardware transfer."});
    std::fs::write(format!("{}/validation.json",args[1]),serde_json::to_vec_pretty(&report)?)?;eprintln!("{report}");Ok(())
}

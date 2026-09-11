//! Learn trajectory residuals from committed native captures; Rust owns labels,
//! normalization, kinematic references, neural gradients and validation metrics.
use sim_runtime::motion_forecast::*;
use serde::{Serialize,Deserialize};
use serde_json::Value;
use std::{fs,path::Path};
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureWindow {path:String,start_s:f64,end_s:f64,#[serde(default)] format:DataFormat}
#[derive(Default,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
enum DataFormat {#[default] NativeCapture,SpeedRollout}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Experiment {version:u32,recipe:ForecastRecipe,training:Vec<CaptureWindow>,validation:Vec<CaptureWindow>,width:usize,seed:u64,epochs:usize,batch_size:usize,learning_rate:f64}
fn write(path:impl AsRef<Path>,value:&impl Serialize)->Result<(),Box<dyn std::error::Error>>{
    let f=fs::OpenOptions::new().write(true).create_new(true).open(path)?;serde_json::to_writer(f,value)?;Ok(())
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let args=std::env::args().skip(1).collect::<Vec<_>>();if args.len()!=2{return Err("usage: train_motion_forecast experiment.json fresh-output-directory".into());}
    let bytes=fs::read(&args[0])?;let e:Experiment=serde_json::from_slice(&bytes)?;if e.version!=1{return Err("forecast experiment version must be 1".into());}
    // Refuse an overlapping train/validation window even when identical input
    // files have been copied to different paths. Sample extraction additionally
    // keeps every history/target window inside its declared interval.
    let hashes=|windows:&[CaptureWindow]|->Result<Vec<_>,Box<dyn std::error::Error>> {
        windows.iter().map(|w|Ok(blake3::hash(&fs::read(&w.path)?))).collect()
    };
    let training_hashes=hashes(&e.training)?;let validation_hashes=hashes(&e.validation)?;
    for (a,ha) in e.training.iter().zip(&training_hashes) { for (b,hb) in e.validation.iter().zip(&validation_hashes) {
        if ha==hb && a.start_s.max(b.start_s)<=a.end_s.min(b.end_s) {
            return Err("training and validation windows overlap in the same capture".into());
        }
    }}
    e.recipe.validate()?;let root=Path::new(&args[1]);fs::create_dir(root)?;fs::write(root.join("experiment.json"),bytes)?;
    let collect=|windows:&[CaptureWindow]|->Result<Vec<ForecastSample>,Box<dyn std::error::Error>>{
        let mut samples=vec![];for w in windows {let raw=fs::read(&w.path)?;
            samples.extend(match w.format {
                DataFormat::NativeCapture=>{let capture:Value=serde_json::from_slice(&raw)?;samples_from_capture(&capture,&e.recipe,w.start_s,w.end_s)?},
                DataFormat::SpeedRollout=>{let rollout=serde_json::from_slice(&raw)?;samples_from_speed_rollout(&rollout,&e.recipe,w.start_s,w.end_s)?},
            });}
        if samples.is_empty(){return Err("empty forecast dataset".into());}Ok(samples)
    };
    let training=collect(&e.training)?;let validation=collect(&e.validation)?;
    let mut model=TrajectoryForecaster::initialize(e.recipe,&training,e.width,e.seed)?;
    let initial_train=model.normalized_loss(&training)?;let initial_validation=model.normalized_loss(&validation)?;
    write(root.join("initial-model.json"),&model)?;
    eprintln!("forecast training: {} samples, {} validation, {} inputs, {} outputs",training.len(),validation.len(),model.network.features.len(),model.network.outputs.len());
    let losses=model.fit(&training,e.epochs,e.batch_size,e.learning_rate)?;
    let final_validation=model.normalized_loss(&validation)?;
    let mut prediction_sse=vec![0.;model.network.outputs.len()];let mut prior_sse=prediction_sse.clone();
    for s in &validation{let prediction=model.predict(&s.inputs,&s.prior)?;
        for i in 0..prediction.len(){prediction_sse[i]+=(prediction[i]-s.targets[i]).powi(2);prior_sse[i]+=(s.prior[i]-s.targets[i]).powi(2);}}
    let per_output=model.network.outputs.iter().enumerate().map(|(i,o)|serde_json::json!({"name":o.target,"unit":o.kind.unit(),
        "prediction_rmse":(prediction_sse[i]/validation.len() as f64).sqrt(),"kinematic_prior_rmse":(prior_sse[i]/validation.len() as f64).sqrt()})).collect::<Vec<_>>();
    write(root.join("model.json"),&model)?;
    let report=serde_json::json!({"training_samples":training.len(),"validation_samples":validation.len(),"initial_training_normalized_mse":initial_train,
        "initial_validation_normalized_mse":initial_validation,"training_losses":losses,"final_validation_normalized_mse":final_validation,"per_output":per_output,
        "scope":"Action-conditioned position, velocity and finite-interval acceleration prediction. Selected kinematic reference plus learned residual; not a reduced-order force/contact planner or PLANC reproduction. Normalization uses training data only. Future actions are known supervision, not privileged future state inputs. This result alone does not establish improved control or transfer."});
    write(root.join("report.json"),&report)?;eprintln!("validation normalized MSE: {initial_validation} -> {final_validation}");Ok(())
}

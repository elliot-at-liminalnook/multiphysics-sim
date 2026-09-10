//! Compare learned forecasts with multiple elementary prediction baselines.
use sim_runtime::motion_forecast::*;
use std::fs;
fn main()->Result<(),Box<dyn std::error::Error>>{
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    let rollout=args.len()==6&&args[5]=="--speed-rollout";
    if args.len()!=5&&!rollout{return Err("usage: evaluate_motion_forecast model.json capture.json start_s end_s output.json [--speed-rollout]".into());}
    let model:TrajectoryForecaster=serde_json::from_slice(&fs::read(&args[0])?)?;
    model.validate()?;
    let raw=fs::read(&args[1])?;
    let samples=if rollout{
        samples_from_speed_rollout(&serde_json::from_slice(&raw)?,&model.recipe,args[2].parse()?,args[3].parse()?)?
    }else{
        samples_from_capture(&serde_json::from_slice(&raw)?,&model.recipe,args[2].parse()?,args[3].parse()?)?
    };
    let mut sums=vec![[0.;4];model.network.outputs.len()];let mut normalized=[0.;4];
    for s in &samples{
        let prediction=model.predict(&s.inputs,&s.prior)?;
        for (horizon,h) in model.recipe.horizons_steps.iter().enumerate(){let t=*h as f64*model.recipe.period_s;
            for axis in 0..model.recipe.axes.len(){let p=s.inputs[axis*3];let v=s.inputs[axis*3+1];let a=s.inputs[axis*3+2];
                let ca=[p+v*t+0.5*a*t*t,v+a*t,a];let cv=[p+v*t,v,0.];let hold=[p,v,a];
                for k in 0..3{let i=horizon*model.recipe.axes.len()*3+axis*3+k;
                    for (method,y) in [prediction[i],ca[k],cv[k],hold[k]].iter().enumerate(){
                        let error=(y-s.targets[i]).powi(2);sums[i][method]+=error;normalized[method]+=error/model.network.outputs[i].scale.powi(2);
                    }
                }
            }
        }
    }
    let names=["learned","constant_acceleration","constant_velocity","last_observation"];
    let rows=model.network.outputs.iter().enumerate().map(|(i,o)|{
        let mut row=serde_json::json!({"name":o.target,"unit":o.kind.unit()});
        for j in 0..4{row[names[j]]=serde_json::json!((sums[i][j]/samples.len() as f64).sqrt());}row
    }).collect::<Vec<_>>();
    let mut scores=serde_json::json!({});for j in 0..4{scores[names[j]]=serde_json::json!(normalized[j]/(samples.len()*rows.len()) as f64);}
    let report=serde_json::json!({"samples":samples.len(),"normalized_mse":scores,"per_output_rmse":rows,
        "scope":"All methods evaluated on the same future windows, with training-only residual scales. Learned model is conditioned on recorded future action sequences. Baselines diagnose predictive value; no claim of closed-loop speed improvement."});
    let file=fs::OpenOptions::new().create_new(true).write(true).open(&args[4])?;serde_json::to_writer(file,&report)?;
    println!("{}",report["normalized_mse"]);Ok(())
}

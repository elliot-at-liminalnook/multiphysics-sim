//! Search a causal learned trajectory model, without substituting its prediction
//! for a measured physics result. All robot-specific choices are input data.
use serde::Deserialize;
use sim_domain_control::optimization::{ProjectedAscentSettings,projected_ascent};
use sim_runtime::motion_forecast::TrajectoryForecaster;
use std::{collections::BTreeMap,fs,io::Write};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Experiment {
    version:u32,model:TrajectoryForecaster,inputs:Vec<f64>,prior:Vec<f64>,
    /// Dimensionless weights on outputs sharing one physical quantity kind.
    output_weights:BTreeMap<String,f64>,
    /// Explicit actuator target bounds, radians, keyed by recipe target name.
    action_bounds:BTreeMap<String,[f64;2]>,optimizer:ProjectedAscentSettings,
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let args=std::env::args().skip(1).collect::<Vec<_>>();if args.len()!=2{return Err("usage: search_forecast_actions experiment.json fresh-result.json".into());}
    let raw=fs::read(&args[0])?;let e:Experiment=serde_json::from_slice(&raw)?;e.model.validate()?;
    if e.version!=1||e.output_weights.is_empty()||e.action_bounds.len()!=e.model.recipe.actuator_targets.len(){return Err("invalid forecast search experiment".into());}
    e.model.predict(&e.inputs,&e.prior)?;
    let mut derivative=vec![0.;e.model.network.outputs.len()];let mut kind=None;
    for (name,weight)in &e.output_weights{
        let i=e.model.network.outputs.iter().position(|o|&o.target==name).ok_or("unknown forecast objective output")?;
        let k=e.model.network.outputs[i].kind;if kind.is_some_and(|old|old!=k)||!weight.is_finite(){return Err("objective outputs need matching physical units and finite weights".into());}kind=Some(k);derivative[i]=*weight;
    }
    if derivative.iter().all(|w|*w==0.){return Err("empty weighted objective".into());}
    let mut indices=vec![];let mut bounds=vec![];
    for step in 1..=*e.model.recipe.horizons_steps.last().unwrap(){for target in &e.model.recipe.actuator_targets{
        let name=format!("action.{step}.{target}");indices.push(e.model.network.features.iter().position(|f|f.source==name).ok_or("missing action feature")?);
        bounds.push(*e.action_bounds.get(target).ok_or("missing explicit action bounds")?);
    }}
    let initial=indices.iter().map(|&i|e.inputs[i]).collect::<Vec<_>>();
    let evaluate=|a:&[f64]|->Result<(f64,Vec<f64>),String>{let mut x=e.inputs.clone();for(&i,&v)in indices.iter().zip(a){x[i]=v;}
        let prediction=e.model.predict(&x,&e.prior)?;let score=prediction.iter().zip(&derivative).map(|(y,w)|y*w).sum();let g=e.model.input_gradient(&x,&derivative)?;
        Ok((score,indices.iter().map(|&i|g[i]).collect()))};
    let result=projected_ascent(&initial,&bounds,&e.optimizer,evaluate)?;
    let mut x=e.inputs.clone();for(&i,&v)in indices.iter().zip(&result.parameters){x[i]=v;}
    let output=serde_json::json!({"version":1,"input_path":args[0],"objective_kind":kind,"initial_prediction":e.model.predict(&e.inputs,&e.prior)?,"optimized_prediction":e.model.predict(&x,&e.prior)?,
        "future_targets_rad":result.parameters.chunks(e.model.recipe.actuator_targets.len()).collect::<Vec<_>>(),"optimization":result,
        "scope":"Local learned-model action-sequence proposal. Current state, prior and past action fixed; all future targets optimized within explicit actuator bounds. This is not a physical rollout, fall check, measured speed or global optimum."});
    let mut f=fs::OpenOptions::new().write(true).create_new(true).open(&args[1])?;serde_json::to_writer_pretty(&mut f,&output)?;f.write_all(b"\n")?;Ok(())
}

//! Speed-policy rollouts and PPO updates through the shared environment.
//! CAD physics, actuator execution and replay stay in EmbeddedEnvironment.
use crate::{embedded::Config, environment::{EmbeddedEnvironment,EnvironmentRecording,Task,Transition},session::Scene};
use serde::{Deserialize,Serialize};
use sim_domain_control::{neural::{Network,Feature,Output,Layer},ppo::*};
use sim_core::QuantityKind;

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct RolloutStep {
    pub time_s:f64,
    pub decision:GaussianDecision,
    pub critic_inputs:Vec<f64>,
    pub values:[f64;2],
    pub reward_m:f64,
    pub fall_cost:f64,
    pub motion_before:crate::motion_data::MotionSnapshot,
    pub motion_after:crate::motion_data::MotionSnapshot,
    /// Actual actuator targets applied over this interval, after saturation.
    pub applied_targets_rad:Vec<f64>,
}
#[derive(Clone,Serialize,Deserialize)]
pub struct SpeedRollout {
    pub steps:Vec<RolloutStep>,
    pub final_transition:Transition,
    pub recording:EnvironmentRecording,
    pub error:Option<String>,
    pub motion_contract:serde_json::Value,
}

/// Two linear value outputs: remaining net progress (m) and eventual fall cost.
/// Privileged critic extras are dx/horizon, dy/horizon and remaining time fraction.
/// They never enter the actor. No output bound limits predicted speed or return.
pub fn initial_critic(actor:&Network)->Result<Network,String>{
    actor.validate()?;
    let mut critic=actor.clone();
    for(name,kind)in [("task.dx_per_horizon",QuantityKind::LinearVelocity),("task.dy_per_horizon",QuantityKind::LinearVelocity),("task.remaining_fraction",QuantityKind::Dimensionless)]{
        critic.features.push(Feature{source:name.into(),subtract:None,kind,center:0.,scale:1.,clip:f64::MAX});
    }
    for row in &mut critic.layers[0].weights {row.extend([0.;3]);}
    critic.outputs=vec![Output{target:"return.net_progress".into(),kind:QuantityKind::Length,scale:1.},Output{target:"return.fall".into(),kind:QuantityKind::Dimensionless,scale:1.}];
    let width=critic.layers.last().unwrap().weights[0].len();
    *critic.layers.last_mut().unwrap()=Layer{weights:vec![vec![0.;width];2],biases:vec![0.;2]};
    critic.validate()?;Ok(critic)
}

pub fn collect_speed_episode(scene:Scene,mut config:Config,task:Task,actions:&[Vec<f64>],actor:&Network,critic:&Network,
    exploration:&GaussianExploration,seed:u64)->Result<SpeedRollout,String>{
    let horizon=config.step_s*config.steps as f64;
    if task.speed.is_none() || (task.period_s-scene.period_s).abs()>1e-12
        || actions.len()!=(horizon/task.period_s).round() as usize {
        return Err("PPO speed rollout requires its distance task, one policy decision per transition and complete action schedule".into());
    }
    critic.validate()?;
    if critic.features.len()!=actor.features.len()+3||critic.outputs.len()!=2 {return Err("PPO critic input/output shape mismatch".into());}
    let policy=config.policy.as_mut().ok_or("PPO requires explicit neural policy")?;
    policy.neural_residual=Some(actor.clone());policy.neural_command_saturation=true;policy.neural_exploration=Some(exploration.clone());
    let mut env=EmbeddedEnvironment::new(scene,config,task,seed)?;
    let mut steps=vec![];let mut error=None;
    for action in actions {
        if env.transition().terminated||env.transition().truncated {break;}
        let previous=env.transition().clone();
        let motion_before=crate::motion_data::MotionSnapshot::from_frame(&env.frame()?)?;
        match env.step(action) {
            Err(e)=>{error=Some(e);break;},
            Ok(t)=>{
                let frame=env.frame()?;
                let sample_time=frame["policy"]["time_s"].as_f64().ok_or("missing policy sample time")?;
                if (sample_time-previous.time_s).abs()>1e-8 {return Err("PPO transition does not match a single held policy decision".into());}
                let decision:GaussianDecision=serde_json::from_value(frame["policy"]["neural_decision"].clone()).map_err(|e|e.to_string())?;
                // Verify behavior-policy likelihood against the exact frozen actor.
                let means=actor.normalized_output(&decision.inputs,false)?;
                if means!=decision.means || exploration.log_probability(&means,&decision.raw_actions)?!=decision.log_probability {
                    return Err("PPO behavior-policy likelihood mismatch".into());
                }
                let mut critic_inputs=decision.inputs.clone();
                let s=previous.speed.as_ref().ok_or("missing speed state")?;
                critic_inputs.extend([s.displacement_xy_m[0]/horizon,s.displacement_xy_m[1]/horizon,(horizon-previous.time_s)/horizon]);
                let values=critic.normalized_output(&critic_inputs,true)?;
                let motion_after=crate::motion_data::MotionSnapshot::from_frame(&frame)?;
                let targets=&frame["policy"]["targets"];
                let applied_targets_rad=actor.outputs.iter().map(|o|targets[&o.target].as_f64().ok_or("missing applied target".into())).collect::<Result<Vec<_>,String>>()?;
                steps.push(RolloutStep{time_s:previous.time_s,decision,critic_inputs,values:[values[0],values[1]],
                    reward_m:t.reward,fall_cost:if t.terminated {1.}else{0.},motion_before,motion_after,applied_targets_rad});
            }
        }
    }
    if error.is_none()&&!env.transition().terminated&&!env.transition().truncated {error=Some("incomplete PPO rollout".into());}
    let motion_contract=serde_json::json!({"frame_coordinates":env.metadata()["frame_coordinates"],
        "links":"named world-frame positions, velocities, angular velocities and rotation matrices",
        "acceleration":"finite differences over recorded intervals; estimates, not instantaneous measured accelerations",
        "actions":"applied_targets_rad are held actuator targets in actor.outputs order; GaussianDecision stores pre-saturation policy actions"});
    Ok(SpeedRollout{steps,final_transition:env.transition().clone(),recording:env.episode_recording(),error,motion_contract})
}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PpoSettings {
    pub epochs:usize,
    pub batch_size:usize,
    pub actor_learning_rate:f64,
    pub critic_learning_rate:f64,
    pub clip_ratio:f64,
    pub maximum_gradient_norm:f64,
    /// Dual ascent learning rate, in metres per observed episode fall rate.
    pub fall_multiplier_learning_rate:f64,
    /// Omit to reproduce legacy cyclic batches; set for an independent seeded
    /// sample permutation per update and epoch, shared by actor and critic.
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub shuffle_seed:Option<u64>,
}
impl PpoSettings {
    pub fn validate(&self)->Result<(),String>{
        if self.epochs==0||self.batch_size==0||[self.actor_learning_rate,self.critic_learning_rate,self.maximum_gradient_norm,self.fall_multiplier_learning_rate]
            .iter().any(|v|!v.is_finite()||*v<=0.)||!self.clip_ratio.is_finite()||self.clip_ratio<=0.||self.clip_ratio>=1.{return Err("invalid PPO settings".into());}Ok(())
    }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct PpoState {pub actor:Network,pub critic:Network,pub actor_adam:Adam,pub critic_adam:Adam,pub fall_multiplier:f64,pub updates:usize}
impl PpoState {
    pub fn new(actor:Network)->Result<Self,String>{let critic=initial_critic(&actor)?;
        Ok(Self{actor_adam:Adam::new(actor.parameters().len()),critic_adam:Adam::new(critic.parameters().len()),actor,critic,fall_multiplier:0.,updates:0})}
    pub fn validate(&self)->Result<(),String>{
        self.actor.validate()?;self.critic.validate()?;
        self.actor_adam.validate(&self.actor.parameters())?;self.critic_adam.validate(&self.critic.parameters())?;
        let expected=initial_critic(&self.actor)?;
        if serde_json::to_value((&self.critic.features,&self.critic.outputs)).map_err(|e|e.to_string())?
            !=serde_json::to_value((&expected.features,&expected.outputs)).map_err(|e|e.to_string())?
            ||!self.fall_multiplier.is_finite()||self.fall_multiplier<0.||self.updates==usize::MAX{
            return Err("invalid saved PPO critic contract, multiplier or update counter".into());
        }Ok(())
    }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct UpdateReport {pub episodes:usize,pub transitions:usize,pub fall_rate:f64,pub fall_multiplier:f64,pub actor_losses:Vec<LossReport>,pub critic_losses:Vec<f64>,
    #[serde(default)] pub exact_policy_kl:Vec<f64>}

/// PPO-Lagrangian with gamma=lambda=1 to preserve the exact finite-horizon
/// distance objective. The only cost is a sampled fall, with target rate zero.
/// This training surrogate does not certify feasibility: validate the resulting
/// deterministic policy over the complete horizon before selecting it.
pub fn update_speed_policy(state:&mut PpoState,rollouts:&[SpeedRollout],exploration:&GaussianExploration,settings:&PpoSettings)->Result<UpdateReport,String>{
    settings.validate()?;
    state.validate()?;
    if rollouts.is_empty()||rollouts.iter().any(|r|r.error.is_some()||r.steps.is_empty()||(!r.final_transition.terminated&&!r.final_transition.truncated)) {
        return Err("PPO needs complete or physically terminated rollouts; numerical failures are not training samples".into());
    }
    if !state.fall_multiplier.is_finite()||state.fall_multiplier<0. {return Err("invalid fall multiplier".into());}
    let fall_rate=rollouts.iter().filter(|r|r.final_transition.terminated).count() as f64/rollouts.len() as f64;
    let multiplier=state.fall_multiplier+settings.fall_multiplier_learning_rate*fall_rate;
    if !multiplier.is_finite(){return Err("fall multiplier overflow".into());}
    let mut policy_samples=vec![];let mut value_samples=vec![];
    for r in rollouts {
        let reward=r.steps.iter().map(|s|s.reward_m).collect::<Vec<_>>();
        let cost=r.steps.iter().map(|s|s.fall_cost).collect::<Vec<_>>();
        let rv=r.steps.iter().map(|s|s.values[0]).collect::<Vec<_>>();let cv=r.steps.iter().map(|s|s.values[1]).collect::<Vec<_>>();
        let ra=advantages(&reward,&rv,0.,1.,1.)?;let ca=advantages(&cost,&cv,0.,1.,1.)?;
        for (i,s) in r.steps.iter().enumerate(){
            let current=state.actor.normalized_output(&s.decision.inputs,false)?;
            if current!=s.decision.means||exploration.log_probability(&current,&s.decision.raw_actions)?!=s.decision.log_probability {
                return Err("PPO data must come from the current frozen behavior policy".into());
            }
            if state.critic.normalized_output(&s.critic_inputs,true)?.as_slice()!=s.values.as_slice(){return Err("PPO data must match its recorded critic state".into());}
            policy_samples.push(PolicySample{inputs:s.decision.inputs.clone(),raw_actions:s.decision.raw_actions.clone(),old_log_probability:s.decision.log_probability,advantage:ra[i]-multiplier*ca[i]});
            value_samples.push(ValueSample{inputs:s.critic_inputs.clone(),targets:vec![ra[i]+rv[i],ca[i]+cv[i]]});
        }
    }
    let mean=policy_samples.iter().map(|s|s.advantage).sum::<f64>()/policy_samples.len() as f64;
    let scale=(policy_samples.iter().map(|s|(s.advantage-mean).powi(2)).sum::<f64>()/policy_samples.len() as f64).sqrt().max(1e-8);
    for s in &mut policy_samples{s.advantage=(s.advantage-mean)/scale;}
    let mut next=state.clone();next.fall_multiplier=multiplier;
    let mut report=UpdateReport{episodes:rollouts.len(),transitions:policy_samples.len(),fall_rate,fall_multiplier:multiplier,actor_losses:vec![],critic_losses:vec![],exact_policy_kl:vec![]};
    for epoch in 0..settings.epochs {
        let order=settings.shuffle_seed.map_or_else(||(0..policy_samples.len()).collect::<Vec<_>>(),|seed|
            sim_domain_control::optimization::seeded_permutation(policy_samples.len(),seed.wrapping_add((state.updates as u64).wrapping_mul(0x9e3779b97f4a7c15)).wrapping_add(epoch as u64)));
        let count=policy_samples.len().div_ceil(settings.batch_size);
        for j in 0..count {
            let i=if settings.shuffle_seed.is_some(){j}else{(j+epoch)%count};let start=i*settings.batch_size;let end=(start+settings.batch_size).min(policy_samples.len());
            let batch=order[start..end].iter().map(|&i|policy_samples[i].clone()).collect::<Vec<_>>();
            let values=order[start..end].iter().map(|&i|value_samples[i].clone()).collect::<Vec<_>>();
            let(_,g)=policy_gradient(&next.actor,exploration,&batch,settings.clip_ratio)?;
            next.actor=next.actor.with_parameters(&next.actor_adam.step(&next.actor.parameters(),&g,settings.actor_learning_rate,Some(settings.maximum_gradient_norm))?)?;
            let(_,g)=value_gradient(&next.critic,&values)?;
            next.critic=next.critic.with_parameters(&next.critic_adam.step(&next.critic.parameters(),&g,settings.critic_learning_rate,Some(settings.maximum_gradient_norm))?)?;
        }
        report.actor_losses.push(policy_gradient(&next.actor,exploration,&policy_samples,settings.clip_ratio)?.0);
        report.critic_losses.push(value_gradient(&next.critic,&value_samples)?.0);
        let kl=rollouts.iter().flat_map(|r|&r.steps).map(|s|exploration.kl(&s.decision.means,&next.actor.normalized_output(&s.decision.inputs,false)?)).collect::<Result<Vec<_>,String>>()?;
        report.exact_policy_kl.push(kl.iter().sum::<f64>()/kl.len() as f64);
    }
    next.updates+=1;*state=next;Ok(report)
}

//! PPO-Clip math and seeded Gaussian exploration. No robot or physics logic.
//! Likelihoods concern raw normalized actions, before actuator saturation.
use crate::neural::Network;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GaussianExploration { pub standard_deviation: Vec<f64> }
impl GaussianExploration {
    pub fn validate(&self, width: usize) -> Result<(),String> {
        if width==0 || self.standard_deviation.len()!=width || self.standard_deviation.iter().any(|s|!s.is_finite()||*s<=0.) {
            return Err("Gaussian exploration requires one finite positive standard deviation per neural output".into());
        }
        Ok(())
    }
    pub fn log_probability(&self, mean: &[f64], action: &[f64]) -> Result<f64,String> {
        self.validate(mean.len())?;
        if mean.len()!=action.len() || mean.iter().chain(action).any(|x|!x.is_finite()) {return Err("invalid Gaussian vectors".into());}
        let value=mean.iter().zip(action).zip(&self.standard_deviation).map(|((m,a),s)|
            -0.5*((a-m)/s).powi(2)-s.ln()-0.5*(2.*std::f64::consts::PI).ln()).sum::<f64>();
        if !value.is_finite() {return Err("nonfinite Gaussian likelihood".into());}
        Ok(value)
    }
    /// Exact KL between two means with this same diagonal covariance, in raw
    /// normalized action coordinates before actuator saturation.
    pub fn kl(&self,old_mean:&[f64],new_mean:&[f64])->Result<f64,String>{
        self.validate(old_mean.len())?;
        if old_mean.len()!=new_mean.len()||old_mean.iter().chain(new_mean).any(|x|!x.is_finite()){return Err("invalid Gaussian KL means".into());}
        let kl=old_mean.iter().zip(new_mean).zip(&self.standard_deviation).map(|((a,b),s)|0.5*((b-a)/s).powi(2)).sum::<f64>();
        if !kl.is_finite(){return Err("nonfinite Gaussian KL".into());}Ok(kl)
    }
}

pub struct GaussianSampler { config: GaussianExploration, state: crate::optimization::SplitMix64 }
impl GaussianSampler {
    pub fn new(config: GaussianExploration, width: usize, seed: u64) -> Result<Self,String> {
        config.validate(width)?; Ok(Self{config,state:crate::optimization::SplitMix64::new(seed)})
    }
    fn uniform(&mut self) -> f64 {
        // Open interval (0,1), with 52 bits so rounding cannot yield 1.
        ((self.state.next()>>12) as f64+0.5)/(1u64<<52) as f64
    }
    pub fn sample(&mut self, mean: &[f64]) -> Result<(Vec<f64>,f64),String> {
        self.config.validate(mean.len())?;
        if mean.iter().any(|x|!x.is_finite()) {return Err("nonfinite Gaussian mean".into());}
        let mut actions=Vec::with_capacity(mean.len());
        for (i,m) in mean.iter().enumerate() {
            let z=(-2.*self.uniform().ln()).sqrt()*(2.*std::f64::consts::PI*self.uniform()).cos();
            actions.push(m+self.config.standard_deviation[i]*z);
        }
        let logp=self.config.log_probability(mean,&actions)?;
        Ok((actions,logp))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GaussianDecision {
    pub inputs: Vec<f64>,
    pub means: Vec<f64>,
    pub raw_actions: Vec<f64>,
    pub log_probability: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicySample {
    pub inputs: Vec<f64>,
    pub raw_actions: Vec<f64>,
    pub old_log_probability: f64,
    pub advantage: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LossReport {pub loss: f64, pub approximate_kl: f64, pub clipped_fraction: f64}

pub fn policy_gradient(network: &Network, exploration: &GaussianExploration, samples: &[PolicySample], clip: f64)
    -> Result<(LossReport,Vec<f64>),String> {
    network.validate()?; exploration.validate(network.outputs.len())?;
    if samples.is_empty() || !clip.is_finite() || clip<=0. || clip>=1. {return Err("invalid PPO batch or ratio clipping".into());}
    let mut gradient=vec![0.;network.parameters().len()];
    let mut report=LossReport{loss:0.,approximate_kl:0.,clipped_fraction:0.};
    for sample in samples {
        if !sample.advantage.is_finite() || !sample.old_log_probability.is_finite() {return Err("nonfinite PPO sample".into());}
        let mean=network.normalized_output(&sample.inputs,false)?;
        let logp=exploration.log_probability(&mean,&sample.raw_actions)?;
        let log_ratio=logp-sample.old_log_probability;
        let ratio=log_ratio.exp();
        if !ratio.is_finite() {return Err("nonfinite PPO importance ratio".into());}
        let clipped=(sample.advantage>=0. && ratio>1.+clip)||(sample.advantage<0. && ratio<1.-clip);
        report.loss-=sample.advantage*if clipped {ratio.clamp(1.-clip,1.+clip)} else {ratio};
        report.approximate_kl+=ratio-1.-log_ratio;
        report.clipped_fraction+=if clipped {1.} else {0.};
        if !clipped {
            let derivative=mean.iter().zip(&sample.raw_actions).zip(&exploration.standard_deviation)
                .map(|((m,a),s)|-sample.advantage*ratio*((a-m)/s)/s/samples.len() as f64).collect::<Vec<_>>();
            for (g,v) in gradient.iter_mut().zip(network.output_gradient(&sample.inputs,&derivative,false)?) {*g+=v;}
        }
    }
    report.loss/=samples.len() as f64;report.approximate_kl/=samples.len() as f64;report.clipped_fraction/=samples.len() as f64;
    if !report.loss.is_finite()||!report.approximate_kl.is_finite()||gradient.iter().any(|x|!x.is_finite()) {return Err("nonfinite PPO loss/gradient".into());}
    Ok((report,gradient))
}

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ValueSample {pub inputs:Vec<f64>,pub targets:Vec<f64>}
pub fn value_gradient(network:&Network,samples:&[ValueSample])->Result<(f64,Vec<f64>),String>{
    network.validate()?;
    if samples.is_empty(){return Err("empty value batch".into());}
    let mut loss=0.;let mut gradient=vec![0.;network.parameters().len()];
    let denominator=(samples.len()*network.outputs.len()) as f64;
    for s in samples {
        if s.targets.len()!=network.outputs.len()||s.targets.iter().any(|x|!x.is_finite()){return Err("invalid value targets".into());}
        let values=network.normalized_output(&s.inputs,true)?;
        let derivative=values.iter().zip(&s.targets).map(|(v,t)|2.*(v-t)/denominator).collect::<Vec<_>>();
        loss+=values.iter().zip(&s.targets).map(|(v,t)|(v-t).powi(2)/denominator).sum::<f64>();
        for(g,v)in gradient.iter_mut().zip(network.output_gradient(&s.inputs,&derivative,true)?){*g+=v;}
    }
    if !loss.is_finite()||gradient.iter().any(|v|!v.is_finite()){return Err("nonfinite value loss/gradient".into());}
    Ok((loss,gradient))
}

/// Finite-horizon GAE. Pass bootstrap=0 at true episode ends (including falls).
/// A segment cut inside an episode instead needs its next-state value.
pub fn advantages(rewards:&[f64],values:&[f64],bootstrap:f64,gamma:f64,lambda:f64)->Result<Vec<f64>,String>{
    if rewards.is_empty()||rewards.len()!=values.len()||!bootstrap.is_finite()
        ||[gamma,lambda].iter().any(|v|!v.is_finite()||*v<0.||*v>1.)
        ||rewards.iter().chain(values).any(|v|!v.is_finite()){return Err("invalid GAE trajectory".into());}
    let mut result=vec![0.;values.len()];let mut next=bootstrap;let mut carry=0.;
    for i in (0..values.len()).rev(){carry=rewards[i]+gamma*next-values[i]+gamma*lambda*carry;result[i]=carry;next=values[i];}
    if result.iter().any(|x|!x.is_finite()){return Err("nonfinite advantages".into());}Ok(result)
}

pub use crate::optimization::Adam;

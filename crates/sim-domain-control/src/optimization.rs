//! Shared gradient optimizer state for supervised and reinforcement learning.
use serde::{Serialize,Deserialize};

pub(crate) struct SplitMix64 {state:u64}
impl SplitMix64 {
    pub(crate) fn new(seed:u64)->Self{Self{state:seed}}
    pub(crate) fn next(&mut self)->u64{
        self.state=self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z=self.state;
        z=(z^(z>>30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z=(z^(z>>27)).wrapping_mul(0x94d049bb133111eb);
        z^(z>>31)
    }
}
/// Reproducible Fisher-Yates permutation, with rejection sampling to avoid
/// modulo bias. This RNG is independent of the controller's exploration draws.
pub fn seeded_permutation(length:usize,seed:u64)->Vec<usize>{
    let mut order=(0..length).collect::<Vec<_>>();let mut rng=SplitMix64::new(seed);
    for i in (1..length).rev(){let bound=(i+1) as u64;let threshold=bound.wrapping_neg()%bound;
        let j=loop{let r=rng.next();if r>=threshold{break (r%bound) as usize;}};
        order.swap(i,j);
    }order
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Adam {first:Vec<f64>,second:Vec<f64>,steps:u64}
impl Adam {
    pub fn new(width:usize)->Self{Self{first:vec![0.;width],second:vec![0.;width],steps:0}}
    pub fn validate(&self,parameters:&[f64])->Result<(),String>{
        if parameters.len()!=self.first.len()||self.second.len()!=parameters.len()
            ||parameters.iter().chain(&self.first).chain(&self.second).any(|x|!x.is_finite())
            ||self.second.iter().any(|x|*x<0.)||self.steps==u64::MAX{return Err("invalid saved Adam state".into());}Ok(())
    }
    pub fn step(&mut self,parameters:&[f64],gradient:&[f64],learning_rate:f64,maximum_norm:Option<f64>)->Result<Vec<f64>,String>{
        self.validate(parameters)?;
        if parameters.len()!=self.first.len()||gradient.len()!=parameters.len()||self.second.len()!=parameters.len()
            ||!learning_rate.is_finite()||learning_rate<=0.||parameters.iter().chain(gradient).chain(&self.first).chain(&self.second).any(|x|!x.is_finite())
            ||maximum_norm.is_some_and(|n|!n.is_finite()||n<=0.){return Err("invalid Adam update".into());}
        let norm=gradient.iter().fold(0f64,|s,x|s.hypot(*x));
        let scale=maximum_norm.map_or(1.,|n|if norm>n {n/norm} else {1.});
        let steps=self.steps.checked_add(1).ok_or("Adam step overflow")?;
        let mut next=self.clone();next.steps=steps;
        let mut output=parameters.to_vec();
        for i in 0..output.len(){let g=gradient[i]*scale;
            next.first[i]=0.9*self.first[i]+0.1*g;next.second[i]=0.999*self.second[i]+0.001*g*g;
            output[i]-=learning_rate*(next.first[i]/(1.-0.9f64.powf(steps as f64)))/((next.second[i]/(1.-0.999f64.powf(steps as f64))).sqrt()+1e-8);
        }
        if output.iter().chain(&next.first).chain(&next.second).any(|x|!x.is_finite()){return Err("nonfinite Adam state".into());}
        *self=next;Ok(output)
    }
}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectedAscentSettings {
    pub iterations:usize,
    /// Step length in coordinates normalized by each declared bound width.
    pub step_fraction:f64,
    pub backtracks:usize,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ProjectedAscentResult {
    pub parameters:Vec<f64>,pub objectives:Vec<f64>,pub evaluations:usize,pub stop:String,
}
/// Local bounded ascent with exact caller-provided derivatives and backtracking.
/// Bounds describe the search domain; optimizer stopping is not a global proof.
/// Callback errors are returned, never converted into objective measurements.
pub fn projected_ascent<F>(initial:&[f64],bounds:&[[f64;2]],settings:&ProjectedAscentSettings,mut evaluate:F)->Result<ProjectedAscentResult,String>
where F:FnMut(&[f64])->Result<(f64,Vec<f64>),String>{
    if initial.is_empty()||initial.len()!=bounds.len()||settings.iterations==0||settings.backtracks==0
        ||!settings.step_fraction.is_finite()||settings.step_fraction<=0.
        ||initial.iter().zip(bounds).any(|(x,b)|!x.is_finite()||b.iter().any(|v|!v.is_finite())||b[0]>=b[1]||!((b[1]-b[0]).is_finite())||*x<b[0]||*x>b[1]){
        return Err("invalid projected ascent inputs".into());
    }
    let check=|score:f64,g:&[f64]|if !score.is_finite()||g.len()!=initial.len()||g.iter().any(|v|!v.is_finite()){Err("invalid ascent objective/gradient".to_string())}else{Ok(())};
    let(mut score,mut gradient)=evaluate(initial)?;check(score,&gradient)?;
    let mut out=ProjectedAscentResult{parameters:initial.to_vec(),objectives:vec![score],evaluations:1,stop:"iteration_limit".into()};
    for _ in 0..settings.iterations{
        let scaled=gradient.iter().zip(bounds).map(|(g,b)|g*(b[1]-b[0])).collect::<Vec<_>>();
        if scaled.iter().any(|v|!v.is_finite()){return Err("nonfinite scaled ascent gradient".into());}
        let max=scaled.iter().map(|v|v.abs()).fold(0.,f64::max);
        if max==0.{out.stop="zero_gradient".into();break;}
        let norm=scaled.iter().map(|v|(v/max).powi(2)).sum::<f64>().sqrt();
        let direction=scaled.iter().map(|v|(v/max)/norm).collect::<Vec<_>>();
        let mut accepted=None;let mut step=settings.step_fraction;
        for _ in 0..settings.backtracks{
            let trial=out.parameters.iter().zip(&direction).zip(bounds).map(|((x,d),b)|(x+step*d*(b[1]-b[0])).clamp(b[0],b[1])).collect::<Vec<_>>();
            if trial==out.parameters{break;}
            let(next,g)=evaluate(&trial)?;out.evaluations+=1;check(next,&g)?;
            if next>score{accepted=Some((trial,next,g));break;}step*=0.5;
        }
        if let Some((p,s,g))=accepted{out.parameters=p;score=s;gradient=g;out.objectives.push(score);}else{out.stop="no_improving_projected_step".into();break;}
    }Ok(out)
}

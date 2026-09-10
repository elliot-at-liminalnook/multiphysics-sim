//! Action-conditioned trajectory prediction around a constant-acceleration
//! kinematic reference. Labels are committed runtime motion, not that reference.
use crate::motion_data::MotionSnapshot;
use serde::{Deserialize,Serialize};
use serde_json::Value;
use sim_core::QuantityKind as Q;
use sim_domain_control::{neural::{Network,Feature,Output,Layer},ppo::{ValueSample,GaussianExploration,GaussianSampler,value_gradient},optimization::Adam};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
pub enum MotionAxis {
    Joint {name:String,index:usize,position_kind:Q},
    Link {name:String,link:String,axis:usize},
}
impl MotionAxis {
    fn name(&self)->&str{match self {Self::Joint{name,..}|Self::Link{name,..}=>name}}
    fn kinds(&self)->Result<[Q;3],String>{match self {
        Self::Joint{position_kind:Q::Angle,..}=>Ok([Q::Angle,Q::AngularVelocity,Q::AngularAcceleration]),
        Self::Joint{position_kind:Q::Length,..}|Self::Link{..}=>Ok([Q::Length,Q::LinearVelocity,Q::LinearAcceleration]),
        _=>Err("forecast joint kind must be angle or length".into()),
    }}
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastRecipe {
    pub expected_cad_sha256:String,
    pub reference_link:String,
    pub axes:Vec<MotionAxis>,
    /// Legacy actuator-position action layer. Mutually exclusive with controller inputs.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub actuator_targets:Vec<String>,
    /// Complete typed controller input layer, in the recipe's chosen order.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub controller_inputs:Vec<crate::session::InputChannel>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub controller_context:Option<crate::forecast_actions::ControllerContext>,
    /// Required for controller-conditioned models. Legacy actuator models may
    /// omit this binding, but cannot claim checked physics/source compatibility.
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub physics_context:Option<crate::physics_context::PhysicsContext>,
    pub horizons_steps:Vec<usize>,
    pub period_s:f64,
    #[serde(default,skip_serializing_if="KinematicReference::is_default")]
    pub reference:KinematicReference,
    /// Link COM height above the authored world surface at its current XY.
    /// This is an ideal runtime observation, not hull clearance or a contact flag.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub terrain_relative_links:Vec<String>,
    /// Current authored IMU channels, availability and age. No future sensor
    /// measurements enter the action-conditioned input window.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub imu_observations:Vec<String>,
}
#[derive(Clone,Debug,Default,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum KinematicReference {#[default] ConstantAcceleration,ConstantVelocity}
impl KinematicReference {fn is_default(&self)->bool{matches!(self,Self::ConstantAcceleration)}}
impl ForecastRecipe {
    pub fn validate(&self)->Result<(),String>{
        let unique=|names:Vec<&str>|names.iter().all(|n|!n.trim().is_empty())&&names.iter().collect::<std::collections::BTreeSet<_>>().len()==names.len();
        if self.expected_cad_sha256.is_empty()||self.reference_link.is_empty()||self.axes.is_empty()
            ||self.actuator_targets.is_empty()==self.controller_inputs.is_empty()
            ||!self.period_s.is_finite()||self.period_s<=0.||self.horizons_steps.is_empty()
            ||self.horizons_steps.iter().any(|h|*h==0)||self.horizons_steps.windows(2).any(|h|h[0]>=h[1])
            ||!unique(self.axes.iter().map(|a|a.name()).collect())||!unique(self.actuator_targets.iter().map(String::as_str).collect())
            ||!unique(self.terrain_relative_links.iter().map(String::as_str).collect())
            ||!unique(self.imu_observations.iter().map(String::as_str).collect()){
            return Err("invalid trajectory forecast recipe".into());
        }
        for a in &self.axes {a.kinds()?;if matches!(a,MotionAxis::Link{axis,..} if *axis>=3){return Err("invalid Cartesian axis".into());}}
        match (&self.controller_context,self.controller_inputs.is_empty()) {
            (None,true)=>{},(Some(context),false)=>context.validate(&self.controller_inputs)?,
            _=>return Err("controller forecasts require their explicit controller context; actuator forecasts must omit it".into()),
        }
        if let Some(context)=&self.physics_context {context.validate()?;}
        else if !self.controller_inputs.is_empty(){return Err("controller forecast requires explicit physics context".into());}
        Ok(())
    }
    pub fn validate_physics_context(&self,actual:&crate::physics_context::PhysicsContext)->Result<(),String>{
        self.validate()?;
        if let Some(expected)=&self.physics_context {expected.matches(actual)?;}
        Ok(())
    }
    pub(crate) fn channels(&self)->Result<(Vec<(String,Q)>,Vec<(String,Q)>),String>{
        self.validate()?;let mut inputs=vec![];let mut outputs=vec![];
        for a in &self.axes {for (suffix,kind) in ["position","velocity","acceleration_estimate"].iter().zip(a.kinds()?){inputs.push((format!("{}.{}",a.name(),suffix),kind));}}
        for axis in ["x","y","z"]{inputs.push((format!("body.gravity.{axis}"),Q::Dimensionless));}
        for axis in ["x","y","z"]{inputs.push((format!("body.angular_velocity.{axis}"),Q::AngularVelocity));}
        for link in &self.terrain_relative_links{inputs.push((format!("terrain.{link}.height"),Q::Length));}
        for name in &self.imu_observations {for c in crate::imu_observation::ImuChannel::ALL {
            inputs.push((format!("imu.{name}.{}",c.suffix()),c.kind()));}}
        for step in 0..=*self.horizons_steps.last().unwrap(){for (target,kind) in self.action_channels() {inputs.push((format!("action.{step}.{target}"),kind));}}
        for h in &self.horizons_steps{for a in &self.axes{for (suffix,kind) in ["position","velocity","acceleration_estimate"].iter().zip(a.kinds()?){outputs.push((format!("h{h}.{}.{suffix}",a.name()),kind));}}}
        Ok((inputs,outputs))
    }
    /// Index of the first future action, after state and previous applied targets.
    pub fn future_action_offset(&self)->usize{
        self.axes.len()*3+6+self.terrain_relative_links.len()+8*self.imu_observations.len()+self.action_count()
    }
    pub fn action_count(&self)->usize{self.actuator_targets.len()+self.controller_inputs.len()}
    pub fn action_channels(&self)->Vec<(&str,Q)>{
        if self.controller_inputs.is_empty(){self.actuator_targets.iter().map(|n|(n.as_str(),Q::Angle)).collect()}
        else {self.controller_inputs.iter().map(|c|(c.name.as_str(),c.kind)).collect()}
    }
    /// Resolve named state and sensor channels against the actual compiled CAD
    /// robot. This does not establish calibration or identify world overrides.
    pub fn validate_robot(&self,art:&sim_domain_robot::Articulated)->Result<(),String>{
        self.validate()?;
        if art.model.source["cad_sha256"].as_str()!=Some(&self.expected_cad_sha256)
            ||art.gravity[0]!=0.||art.gravity[1]!=0.||!art.gravity[2].is_finite()||art.gravity[2]>=0. {
            return Err("forecast CAD or gravity contract mismatch".into());
        }
        crate::imu_observation::ImuObserver::new(art,&self.imu_observations)?;
        let has_link=|name:&str|art.links.iter().filter(|l|l.name==name).count()==1;
        if !has_link(&self.reference_link){return Err("missing or ambiguous forecast reference link".into());}
        if !self.terrain_relative_links.is_empty(){
            sim_domain_robot::model::validate_ground_surface(art.floor_z,art.terrain.as_ref())?;
            if self.terrain_relative_links.iter().any(|n|!has_link(n)){return Err("missing terrain-relative forecast link".into());}
        }
        for axis in &self.axes {match axis {
            MotionAxis::Joint{name,index,position_kind}=>{
                let (_,d)=art.dofs().nth(*index).ok_or("unknown forecast coordinate index")?;
                let kind=match d.kind {sim_domain_robot::articulated::DofKind::Revolute=>Q::Angle,
                    sim_domain_robot::articulated::DofKind::Prismatic=>Q::Length};
                if &d.name!=name||&kind!=position_kind{return Err("forecast coordinate index/name/unit mismatch".into());}
            },
            MotionAxis::Link{link,..}=>if !has_link(link){return Err("missing forecast target link".into());},
        }}
        Ok(())
    }
}

fn components(recipe:&ForecastRecipe,snapshot:&MotionSnapshot,anchor:&MotionSnapshot)->Result<Vec<[f64;2]>,String>{
    let body=anchor.poses.iter().find(|l|l.name==recipe.reference_link).ok_or("missing forecast reference link")?;
    recipe.axes.iter().map(|a|match a {
        MotionAxis::Joint{index,..}=>Ok([*snapshot.joint_positions.get(*index).ok_or("missing forecast joint")?,*snapshot.joint_velocities.get(*index).ok_or("missing forecast velocity")?]),
        MotionAxis::Link{link,axis,..}=>{
            let p=snapshot.poses.iter().find(|p|&p.name==link).ok_or("missing forecast link")?;
            Ok([(0..3).map(|j|body.rotation[j][*axis]*(p.position_m[j]-body.position_m[j])).sum(),
                (0..3).map(|j|body.rotation[j][*axis]*p.velocity_m_s[j]).sum()])
        }
    }).collect()
}
/// Predictive state uses a frame anchored at the current reference pose. Future
/// positions and velocities retain that frame; they do not rotate with the robot.
pub fn forecast_input(recipe:&ForecastRecipe,previous:&MotionSnapshot,current:&MotionSnapshot,previous_targets:&[f64],future_targets:&[Vec<f64>])
    ->Result<(Vec<f64>,Vec<f64>),String>{
    recipe.validate()?;let dt=current.time_s-previous.time_s;
    if (dt-recipe.period_s).abs()>1e-8||future_targets.len()!=*recipe.horizons_steps.last().unwrap()
        ||previous_targets.len()!=recipe.action_count()||future_targets.iter().any(|a|a.len()!=previous_targets.len()){
        return Err("forecast requires exact history interval and future action sequence".into());
    }
    if !recipe.controller_inputs.is_empty(){
        for a in std::iter::once(previous_targets).chain(future_targets.iter().map(Vec::as_slice)){
            crate::forecast_actions::validate_values(&recipe.controller_inputs,a)?;
        }
    }
    let now=components(recipe,current,current)?;let prev=components(recipe,previous,current)?;
    let acceleration=now.iter().zip(&prev).map(|(a,b)|(a[1]-b[1])/dt).collect::<Vec<_>>();
    let mut inputs=vec![];for (x,a) in now.iter().zip(&acceleration){inputs.extend([x[0],x[1],*a]);}
    let body=current.poses.iter().find(|p|p.name==recipe.reference_link).ok_or("missing forecast body")?;
    inputs.extend((0..3).map(|i|-body.rotation[2][i]));
    inputs.extend((0..3).map(|i|(0..3).map(|j|body.rotation[j][i]*body.angular_velocity_rad_s[j]).sum::<f64>()));
    for link in &recipe.terrain_relative_links{
        let p=current.poses.iter().find(|p|&p.name==link).ok_or("missing terrain-relative link")?;
        let floor=current.floor_heights_m.get(link).ok_or("missing explicit ground observation")?;
        inputs.push(p.position_m[2]-floor);
    }
    crate::imu_observation::validate_readings(&current.imu_samples,current.time_s)?;
    for name in &recipe.imu_observations {
        let sample=current.imu_samples.iter().find(|s|&s.name==name).ok_or("missing forecast IMU")?;
        for c in crate::imu_observation::ImuChannel::ALL { inputs.push(c.read(sample,current.time_s)?); }
    }
    inputs.extend(previous_targets);for a in future_targets{inputs.extend(a);}
    let mut prior=vec![];
    for h in &recipe.horizons_steps{let t=*h as f64*recipe.period_s;
        for (x,a) in now.iter().zip(&acceleration){
            let a=match recipe.reference {KinematicReference::ConstantAcceleration=>*a,KinematicReference::ConstantVelocity=>0.};
            prior.extend([x[0]+x[1]*t+0.5*a*t*t,x[1]+a*t,a]);}}
    if inputs.iter().chain(&prior).any(|x|!x.is_finite()){return Err("nonfinite forecast inputs/reference".into());}Ok((inputs,prior))
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ForecastSample {pub time_s:f64,pub inputs:Vec<f64>,pub prior:Vec<f64>,pub targets:Vec<f64>}

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ControllerTrajectoryPrediction {
    pub time_s:f64,
    pub physics_context:crate::physics_context::PhysicsContext,
    pub actions:crate::forecast_actions::ControllerActionSequence,
    pub inputs:Vec<f64>,
    pub prior:Vec<f64>,
    pub prediction:Vec<f64>,
}

/// Only use samples whose complete prediction windows lie within [start,end].
/// Command j is read from the following frame: the policy sampled at t_j is
/// held during [t_j,t_(j+1)], matching the production environment timing.
pub fn samples_from_capture(capture:&Value,recipe:&ForecastRecipe,start:f64,end:f64)->Result<Vec<ForecastSample>,String>{
    recipe.validate()?;
    let recording:Option<crate::embedded::EmbeddedRecording>=if recipe.physics_context.is_some()||!recipe.controller_inputs.is_empty(){
        Some(serde_json::from_value(capture["recording"].clone()).map_err(|e|e.to_string())?)
    }else{None};
    if let Some(context)=&recipe.physics_context {
        context.matches(&crate::physics_context::PhysicsContext::from_recording(recording.as_ref().unwrap())?)?;
    }
    if !start.is_finite()||!end.is_finite()||start>=end||capture.get("error")!=Some(&Value::Null)
        ||capture["recording"]["scene"]["robot"]["source"]["cad_sha256"].as_str()!=Some(&recipe.expected_cad_sha256){return Err("invalid forecast capture provenance/window".into());}
    let gravity=&capture["recording"]["scene"]["robot"]["gravity"];
    if gravity[0].as_f64()!=Some(0.)||gravity[1].as_f64()!=Some(0.)||!gravity[2].as_f64().is_some_and(|g|g.is_finite()&&g<0.){
        return Err("forecast gravity-direction features currently require explicit world -Z gravity".into());
    }
    let frames=capture["frames"].as_array().ok_or("missing forecast frames")?;
    for name in &recipe.imu_observations {
        let sensors=capture["recording"]["scene"]["robot"]["sensors"].as_array().ok_or("missing forecast authored sensors")?;
        let matched:Vec<_>=sensors.iter().filter(|s|s["name"].as_str()==Some(name)).collect();
        if matched.len()!=1 || matched[0]["kind"]!="imu" {
            return Err("forecast requires unique authored IMU provenance".into());
        }
        for frame in frames {
            let samples=frame["imu_samples"].as_array().ok_or("missing forecast IMU samples")?;
            if !samples.iter().any(|s|s["name"].as_str()==Some(name)&&s["link"]==matched[0]["link"]) {
                return Err("forecast IMU link provenance mismatch".into());
            }
        }
    }
    for axis in &recipe.axes {if let MotionAxis::Joint{name,index,position_kind}=axis {
        let coordinate=&capture["metadata"]["frame_coordinates"][*index];
        if coordinate["name"].as_str()!=Some(name)||coordinate["position_unit"].as_str()!=Some(position_kind.unit()) {
            return Err("forecast joint index/name/unit provenance mismatch".into());
        }
    }}
    let mut motion=frames.iter().map(MotionSnapshot::from_frame).collect::<Result<Vec<_>,_>>()?;
    if !recipe.terrain_relative_links.is_empty(){
        let raw=&capture["recording"]["scene"]["robot"]["world"];
        if raw["floor_z"].as_f64().is_none()||raw.get("terrain").is_none(){
            return Err("terrain-aware forecast requires explicit recorded floor_z and terrain".into());
        }
        let world:sim_domain_robot::model::World=serde_json::from_value(raw.clone()).map_err(|e|e.to_string())?;
        world.validate_ground()?;
        for snapshot in &mut motion{
            let recorded=snapshot.floor_heights_m.clone();
            snapshot.observe_ground(&recipe.terrain_relative_links,|x,y|world.floor_height(x,y))?;
            for (link,h) in &snapshot.floor_heights_m{
                if recorded.get(link).is_some_and(|old|(old-h).abs()>1e-9){
                    return Err("recorded ground observation disagrees with recorded world".into());
                }
            }
        }
    }
    let actions=if recipe.controller_inputs.is_empty(){
        frames.iter().skip(1).map(|f|recipe.actuator_targets.iter().map(|a|f["policy"]["targets"][a].as_f64().ok_or("missing applied policy target".into())).collect::<Result<Vec<_>,String>>()).collect::<Result<Vec<_>,_>>()?
    }else {
        let recording=recording.as_ref().unwrap();
        recipe.controller_context.as_ref().unwrap().matches(&crate::forecast_actions::ControllerContext::from_runtime(&recording.scene,&recording.config)?)?;
        crate::forecast_actions::from_recording(recording,frames,&recipe.controller_inputs)?
    };
    let max=*recipe.horizons_steps.last().unwrap();let mut samples=vec![];
    for i in 1..motion.len().saturating_sub(max){
        if motion[i-1].time_s<start-1e-9||motion[i+max].time_s>end+1e-9 {continue;}
        if (recipe.controller_inputs.is_empty()&&(0..max).any(|j|frames[i+j+1]["policy"]["time_s"].as_f64().is_none_or(|t|!t.is_finite()||(t-motion[i+j].time_s).abs()>1e-8)))
            ||(1..=max).any(|j|(motion[i+j].time_s-motion[i].time_s-j as f64*recipe.period_s).abs()>1e-8){return Err("forecast capture policy/frame timing mismatch".into());}
        let(inputs,prior)=forecast_input(recipe,&motion[i-1],&motion[i],&actions[i-1],&actions[i..i+max])?;
        let mut targets=vec![];
        for h in &recipe.horizons_steps{let a=components(recipe,&motion[i+h],&motion[i])?;let b=components(recipe,&motion[i+h-1],&motion[i])?;
            for(x,y)in a.iter().zip(&b){targets.extend([x[0],x[1],(x[1]-y[1])/recipe.period_s]);}}
        samples.push(ForecastSample{time_s:motion[i].time_s,inputs,prior,targets});
    }
    if samples.is_empty(){return Err("forecast window contains no complete samples".into());}Ok(samples)
}

/// Adapt actual learner motion and applied targets to the same labelled-data
/// path as native captures. Falling remains valid dynamics; numerical failures
/// and discontinuous history do not become supervised examples.
pub fn samples_from_speed_rollout(rollout:&crate::ppo_training::SpeedRollout,recipe:&ForecastRecipe,start:f64,end:f64)->Result<Vec<ForecastSample>,String>{
    if rollout.error.is_some()||rollout.steps.is_empty()||(!rollout.final_transition.terminated&&!rollout.final_transition.truncated){
        return Err("forecast requires a complete or physically terminated rollout without numerical errors".into());
    }
    let actor=rollout.recording.runtime.config.policy.as_ref().and_then(|p|p.neural_residual.as_ref()).ok_or("rollout has no actuator policy contract")?;
    let mut previous=&rollout.steps[0].motion_before;
    let mut frames=vec![serde_json::to_value(previous).map_err(|e|e.to_string())?];
    for step in &rollout.steps {
        if &step.motion_before!=previous||(step.time_s-step.motion_before.time_s).abs()>1e-8
            ||(step.motion_after.time_s-step.time_s-rollout.recording.task.period_s).abs()>1e-8
            ||step.applied_targets_rad.len()!=actor.outputs.len(){return Err("discontinuous rollout motion or actuator contract".into());}
        let mut frame=serde_json::to_value(&step.motion_after).map_err(|e|e.to_string())?;
        let targets=actor.outputs.iter().zip(&step.applied_targets_rad).map(|(o,v)|(o.target.clone(),*v)).collect::<std::collections::BTreeMap<_,_>>();
        frame["policy"]=serde_json::json!({"time_s":step.time_s,"targets":targets});
        frames.push(frame);previous=&step.motion_after;
    }
    if (previous.time_s-rollout.final_transition.time_s).abs()>1e-8{return Err("rollout final motion/transition mismatch".into());}
    let robot=&rollout.recording.runtime.scene.robot;
    let mut capture=serde_json::json!({"error":null,"frames":frames,"metadata":{"frame_coordinates":rollout.motion_contract["frame_coordinates"]},
        "recording":{"scene":{"robot":{"source":robot.source,"gravity":robot.gravity,"world":robot.world,"sensors":robot.sensors}}}});
    if !recipe.controller_inputs.is_empty()||recipe.physics_context.is_some(){capture["recording"]=serde_json::to_value(&rollout.recording.runtime).map_err(|e|e.to_string())?;}
    samples_from_capture(&capture,recipe,start,end)
}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ForecastOutputActivation {Linear}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct TrajectoryForecaster {pub version:u32,pub recipe:ForecastRecipe,pub network:Network,pub output_activation:ForecastOutputActivation}
impl TrajectoryForecaster {
    pub fn validate(&self)->Result<(),String>{
        let(inputs,outputs)=self.recipe.channels()?;self.network.validate()?;
        let version_matches=matches!((self.version,self.recipe.physics_context.is_some()),(1,false)|(2,true));
        if !version_matches||inputs.len()!=self.network.features.len()||outputs.len()!=self.network.outputs.len()
            ||inputs.iter().zip(&self.network.features).any(|((name,kind),f)|name!=&f.source||kind!=&f.kind||f.subtract.is_some()||f.clip!=f64::MAX)
            ||outputs.iter().zip(&self.network.outputs).any(|((name,kind),o)|name!=&o.target||kind!=&o.kind){return Err("forecast model does not match its typed recipe".into());}
        Ok(())
    }
    /// Fit normalization on training samples only. A zero last layer reproduces
    /// the kinematic reference exactly. Learned residual outputs are unbounded.
    pub fn initialize(recipe:ForecastRecipe,training:&[ForecastSample],width:usize,seed:u64)->Result<Self,String>{
        let(input_channels,output_channels)=recipe.channels()?;
        if training.is_empty()||width==0||training.iter().any(|s|s.inputs.len()!=input_channels.len()||s.targets.len()!=output_channels.len()||s.prior.len()!=s.targets.len()
            ||s.inputs.iter().chain(&s.targets).chain(&s.prior).any(|x|!x.is_finite())){return Err("invalid forecast training matrix".into());}
        let features=input_channels.iter().enumerate().map(|(i,(name,kind))|{
            let center=training.iter().map(|s|s.inputs[i]).sum::<f64>()/training.len() as f64;
            let scale=(training.iter().map(|s|(s.inputs[i]-center).powi(2)).sum::<f64>()/training.len() as f64).sqrt().max(1e-8);
            Feature{source:name.clone(),subtract:None,kind:*kind,center,scale,clip:f64::MAX}
        }).collect::<Vec<_>>();
        let outputs=output_channels.iter().enumerate().map(|(i,(name,kind))|Output{target:name.clone(),kind:*kind,
            scale:(training.iter().map(|s|(s.targets[i]-s.prior[i]).powi(2)).sum::<f64>()/training.len() as f64).sqrt().max(1e-8)}).collect::<Vec<_>>();
        let mut layers=vec![];let mut incoming=features.len();
        for layer in 0..2{let mut rng=GaussianSampler::new(GaussianExploration{standard_deviation:vec![1./(incoming as f64).sqrt();incoming]},incoming,seed+layer)?;
            let weights=(0..width).map(|_|rng.sample(&vec![0.;incoming]).map(|r|r.0)).collect::<Result<Vec<_>,_>>()?;
            layers.push(Layer{weights,biases:vec![0.;width]});incoming=width;}
        layers.push(Layer{weights:vec![vec![0.;width];outputs.len()],biases:vec![0.;outputs.len()]});
        let network=Network{version:1,features,outputs,layers};network.validate()?;
        let version=if recipe.physics_context.is_some(){2}else{1};
        Ok(Self{version,recipe,network,output_activation:ForecastOutputActivation::Linear})
    }
    fn normalize(&self,s:&ForecastSample)->Result<ValueSample,String>{
        if s.inputs.len()!=self.network.features.len()||s.targets.len()!=self.network.outputs.len()||s.prior.len()!=s.targets.len(){return Err("forecast sample shape mismatch".into());}
        Ok(ValueSample{inputs:s.inputs.iter().zip(&self.network.features).map(|(x,f)|(x-f.center)/f.scale).collect(),
            targets:s.targets.iter().zip(&s.prior).zip(&self.network.outputs).map(|((y,p),o)|(y-p)/o.scale).collect()})
    }
    pub fn predict(&self,inputs:&[f64],prior:&[f64])->Result<Vec<f64>,String>{
        self.validate()?;
        if inputs.len()!=self.network.features.len()||prior.len()!=self.network.outputs.len(){return Err("invalid forecast model/input".into());}
        let x=inputs.iter().zip(&self.network.features).map(|(x,f)|(x-f.center)/f.scale).collect::<Vec<_>>();
        let y=self.network.normalized_output(&x,true)?;
        let output=y.iter().zip(&self.network.outputs).zip(prior).map(|((y,o),p)|p+y*o.scale).collect::<Vec<_>>();
        if output.iter().any(|x|!x.is_finite()){return Err("nonfinite forecast".into());}Ok(output)
    }
    /// Physical-output vector-Jacobian product with respect to raw inputs,
    /// holding the explicit kinematic prior fixed. Future actuator commands do
    /// not affect that prior, so their derivatives need no additional term.
    /// State derivatives of the complete forecast require the prior derivative.
    pub fn input_gradient(&self,inputs:&[f64],output_derivative:&[f64])->Result<Vec<f64>,String>{
        self.validate()?;
        if inputs.len()!=self.network.features.len()||output_derivative.len()!=self.network.outputs.len(){return Err("invalid forecast derivative shape".into());}
        let normalized=inputs.iter().zip(&self.network.features).map(|(x,f)|(x-f.center)/f.scale).collect::<Vec<_>>();
        let derivative=output_derivative.iter().zip(&self.network.outputs).map(|(d,o)|d*o.scale).collect::<Vec<_>>();
        let gradient=self.network.input_gradient(&normalized,&derivative,true)?.iter().zip(&self.network.features).map(|(g,f)|g/f.scale).collect::<Vec<_>>();
        if gradient.iter().any(|x|!x.is_finite()){return Err("nonfinite physical forecast derivative".into());}Ok(gradient)
    }
    pub fn normalized_loss(&self,samples:&[ForecastSample])->Result<f64,String>{
        self.validate()?;
        let data=samples.iter().map(|s|self.normalize(s)).collect::<Result<Vec<_>,_>>()?;
        Ok(value_gradient(&self.network,&data)?.0)
    }
    pub fn fit(&mut self,training:&[ForecastSample],epochs:usize,batch_size:usize,learning_rate:f64)->Result<Vec<f64>,String>{
        self.validate()?;
        if epochs==0||batch_size==0{return Err("forecast fit needs epochs and minibatches".into());}
        let data=training.iter().map(|s|self.normalize(s)).collect::<Result<Vec<_>,_>>()?;
        let mut optimizer=Adam::new(self.network.parameters().len());let mut loss=vec![];
        for epoch in 0..epochs{let batches=data.chunks(batch_size).collect::<Vec<_>>();
            for j in 0..batches.len(){let(_,g)=value_gradient(&self.network,batches[(j+epoch)%batches.len()])?;
                self.network=self.network.with_parameters(&optimizer.step(&self.network.parameters(),&g,learning_rate,Some(1.))?)?;}
            loss.push(value_gradient(&self.network,&data)?.0);
        }Ok(loss)
    }
}

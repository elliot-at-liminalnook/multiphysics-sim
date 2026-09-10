//! Physical trajectory data for action-conditioned prediction. No dynamics are
//! reimplemented here; samples come from committed Rust runtime frames.
use serde::{Deserialize,Serialize};
use serde_json::Value;

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct LinkMotion {
    pub name:String,
    pub position_m:[f64;3],
    pub velocity_m_s:[f64;3],
    pub angular_velocity_rad_s:[f64;3],
    pub rotation:[[f64;3];3],
}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct MotionSnapshot {
    pub time_s:f64,
    pub joint_positions:Vec<f64>,
    pub joint_velocities:Vec<f64>,
    pub poses:Vec<LinkMotion>,
    /// Held authored sensor outputs at this snapshot time; None sample time
    /// means unavailable. Sensor values are not reconstructed from ideal motion.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub imu_samples:Vec<sim_domain_robot::articulated::ImuReading>,
    /// Optional world surface heights at link COM XY positions. Old captures
    /// lack these; terrain-aware recipes reconstruct them from the recorded world.
    #[serde(default,skip_serializing_if="std::collections::BTreeMap::is_empty")]
    pub floor_heights_m:std::collections::BTreeMap<String,f64>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct EstimatedAcceleration {
    pub interval_s:f64,
    /// Per frame_coordinates: rad/s² for revolute and m/s² for prismatic axes.
    pub generalized_acceleration:Vec<f64>,
    /// World-frame link accelerations in poses order; finite-interval estimates.
    pub link_linear_m_s2:Vec<[f64;3]>,
    pub link_angular_rad_s2:Vec<[f64;3]>,
}
impl MotionSnapshot {
    /// Same articulated kinematics as runtime frames; no force/contact update.
    pub fn from_state(art:&sim_domain_robot::Articulated,state:&sim_domain_robot::Generalized,time_s:f64)->Self{
        let links=art.evaluate_kinematics_only(state);
        Self{time_s,joint_positions:state.q.clone(),joint_velocities:state.qd.clone(),
            poses:links.iter().zip(&art.model.links).map(|(k,l)|LinkMotion{name:l.name.clone(),position_m:k.p.into(),velocity_m_s:k.vel.into(),
                angular_velocity_rad_s:k.w.into(),rotation:std::array::from_fn(|i|std::array::from_fn(|j|k.r[(i,j)]))}).collect(),
            floor_heights_m:std::collections::BTreeMap::new(),imu_samples:art.imu_readings(state)}
    }
    /// Add requested surface observations without inventing a floor or sensor.
    pub fn observe_ground(&mut self,links:&[String],height:impl Fn(f64,f64)->f64)->Result<(),String>{
        let mut values=std::collections::BTreeMap::new();
        for name in links {
            let p=self.poses.iter().find(|p|&p.name==name).ok_or("missing ground-observation link")?;
            let h=height(p.position_m[0],p.position_m[1]);
            if !h.is_finite(){return Err("nonfinite observed ground height".into());}
            values.insert(name.clone(),h);
        }
        self.floor_heights_m=values;Ok(())
    }
    pub fn from_frame(frame:&Value)->Result<Self,String>{
        let value:Self=serde_json::from_value(frame.clone()).map_err(|e|e.to_string())?;
        crate::imu_observation::validate_readings(&value.imu_samples,value.time_s)?;
        if !value.time_s.is_finite()||value.joint_positions.len()!=value.joint_velocities.len()
            ||value.joint_positions.iter().chain(&value.joint_velocities).any(|x|!x.is_finite())||value.poses.is_empty(){return Err("invalid motion snapshot".into());}
        let mut names=std::collections::BTreeSet::new();
        for p in &value.poses {
            let pose=crate::session::LinkPose{name:p.name.clone(),position_m:p.position_m,rotation:p.rotation};
            if !names.insert(&p.name)||!pose.valid_rigid_transform()||p.velocity_m_s.iter().chain(&p.angular_velocity_rad_s).any(|x|!x.is_finite()){
                return Err("invalid link motion".into());
            }
        }
        if value.floor_heights_m.iter().any(|(name,h)|!h.is_finite()||!names.contains(name)){
            return Err("invalid motion ground observations".into());
        }
        if value.imu_samples.iter().any(|s|!names.contains(&s.link)) {
            return Err("motion IMU names an absent link".into());
        }
        Ok(value)
    }
    pub fn acceleration_since(&self,previous:&Self)->Result<EstimatedAcceleration,String>{
        let dt=self.time_s-previous.time_s;
        if !dt.is_finite()||dt<=0.||self.joint_velocities.len()!=previous.joint_velocities.len()
            ||self.poses.len()!=previous.poses.len()||self.poses.iter().zip(&previous.poses).any(|(a,b)|a.name!=b.name){return Err("motion history time/topology mismatch".into());}
        let value=EstimatedAcceleration{interval_s:dt,
            generalized_acceleration:self.joint_velocities.iter().zip(&previous.joint_velocities).map(|(a,b)|(a-b)/dt).collect(),
            link_linear_m_s2:self.poses.iter().zip(&previous.poses).map(|(a,b)|std::array::from_fn(|i|(a.velocity_m_s[i]-b.velocity_m_s[i])/dt)).collect(),
            link_angular_rad_s2:self.poses.iter().zip(&previous.poses).map(|(a,b)|std::array::from_fn(|i|(a.angular_velocity_rad_s[i]-b.angular_velocity_rad_s[i])/dt)).collect()};
        if value.generalized_acceleration.iter().chain(value.link_linear_m_s2.iter().flatten()).chain(value.link_angular_rad_s2.iter().flatten()).any(|x|!x.is_finite()){
            return Err("nonfinite estimated acceleration".into());
        }Ok(value)
    }
}

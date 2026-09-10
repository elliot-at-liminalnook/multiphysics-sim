//! Net displacement objective, independent of controller and robot topology.
//! Undiscounted transition rewards telescope to endpoint distance in metres.
use crate::session::{LinkPose, Scene};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeedTaskConfig {
    /// CAD link whose origin measures progress and whose hull detects falling.
    pub body_link: String,
}

/// Shared endpoint objective for measurement and predictive control. At the
/// origin the norm is nondifferentiable; zero is one valid subgradient.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetDisplacement {
    pub displacement_xy_m: [f64; 2],
    pub distance_m: f64,
    pub position_gradient: [f64; 2],
}
pub fn net_displacement(origin: [f64; 2], position: [f64; 2]) -> Result<NetDisplacement, String> {
    let s=sim_domain_control::displacement::measure([origin[0],origin[1],0.],
        [position[0],position[1],0.],sim_domain_control::displacement::DisplacementAxes::Xy)?;
    Ok(NetDisplacement{displacement_xy_m:[s.displacement_m[0],s.displacement_m[1]],distance_m:s.distance_m,
        position_gradient:[s.position_gradient[0],s.position_gradient[1]]})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (SpeedMonitor, sim_domain_robot::Articulated) {
        let scene: Scene = serde_json::from_value(json!({"version":1,"period_s":0.02,"duration_s":10.,
            "options":{"contact":true}, "robot":{"gravity":[0.,0.,-9.81],
                "links":[{"name":"body","collision":{"hull":[[-0.1,-0.1,-0.1],[0.1,0.1,0.1]]}}]}})).unwrap();
        let monitor = SpeedMonitor::new(&SpeedTaskConfig { body_link: "body".into() }, &scene, 10.).unwrap();
        let art = sim_domain_robot::Articulated::new(std::sync::Arc::new(scene.robot),
            &sim_domain_robot::articulated::Options::default()).unwrap();
        (monitor, art)
    }
    fn frame(x: f64, y: f64, z: f64) -> Value {
        json!({"poses":[{"name":"body","position_m":[x,y,z],"rotation":[[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]]}],"contacts":[]})
    }
    #[test]
    fn displacement_return_reverses_and_closed_path_has_zero_return() {
        let (mut monitor, art) = fixture();
        let positions = [(10.,20.),(13.,24.),(10.,25.),(7.,24.),(10.,20.)];
        let mut sum = 0.;
        for (x,y) in positions {
            let s = monitor.observe(&frame(x,y,1.), &art).unwrap();
            sum += s.progress_reward_m;
            assert!((sum - (x-10.).hypot(y-20.)).abs() < 1e-12);
            assert_eq!(s.net_speed_m_s, s.net_distance_m / 10.);
            assert!(!s.fallen);
        }
        assert_eq!(sum, 0.);
        assert_eq!(monitor.observe(&frame(13.,24.,1.), &art).unwrap().progress_reward_m, 5.);
        assert_eq!(monitor.observe(&frame(10.,20.,1.), &art).unwrap().progress_reward_m, -5.);
    }
    #[test]
    fn independent_fall_conditions_and_invalid_pose_are_detected() {
        let (mut m, art) = fixture();
        assert!(m.observe(&frame(0.,0.,0.1), &art).unwrap().fallen);
        let mut upside_down = frame(0.,0.,1.);
        upside_down["poses"][0]["rotation"] = json!([[1.,0.,0.],[0.,-1.,0.],[0.,0.,-1.]]);
        assert!(m.observe(&upside_down, &art).unwrap().fallen);
        let mut contact = frame(0.,0.,1.);
        contact["contacts"] = json!([{"link":0,"other":null,"force_n":[0.,0.,1.]}]);
        assert!(m.observe(&contact, &art).unwrap().fallen);
        contact["contacts"][0]["other"] = json!(1);
        assert!(!m.observe(&contact, &art).unwrap().fallen);
        let mut invalid = frame(0.,0.,1.);
        invalid["poses"][0]["rotation"][2][2] = json!(2.);
        assert!(m.observe(&invalid, &art).is_err());
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeedObservation {
    pub displacement_xy_m: [f64; 2],
    pub net_distance_m: f64,
    pub net_speed_m_s: f64,
    pub progress_reward_m: f64,
    pub body_up_z: f64,
    pub hull_floor_clearance_m: f64,
    pub body_floor_contact: bool,
    pub fallen: bool,
}

pub(crate) struct SpeedMonitor {
    link: String,
    index: usize,
    hull: Vec<[f64; 3]>,
    horizon_s: f64,
    origin: Option<[f64; 2]>,
    previous_distance_m: f64,
}

impl SpeedMonitor {
    pub fn new(config: &SpeedTaskConfig, scene: &Scene, horizon_s: f64) -> Result<Self, String> {
        let matches: Vec<_> = scene.robot.links.iter().enumerate()
            .filter(|(_, l)| l.name == config.body_link).collect();
        if matches.len() != 1 || !scene.options.contact || scene.robot.gravity[0] != 0.0
            || scene.robot.gravity[1] != 0.0 || scene.robot.gravity[2] >= 0.0
            || !horizon_s.is_finite() || horizon_s <= 0.0 {
            return Err("speed task requires one named body, contact, world -Z gravity and a positive finite horizon".into());
        }
        let (index, link) = matches[0];
        if link.collision.hull.is_empty() || link.collision.hull.iter().flatten().any(|x| !x.is_finite()) {
            return Err("speed task requires a finite CAD collision hull; no inferred body geometry".into());
        }
        Ok(Self { link: config.body_link.clone(), index, hull: link.collision.hull.clone(),
            horizon_s, origin: None, previous_distance_m: 0.0 })
    }

    pub fn observe(&mut self, frame: &Value, art: &sim_domain_robot::Articulated) -> Result<SpeedObservation, String> {
        let poses: Vec<LinkPose> = serde_json::from_value(frame["poses"].clone()).map_err(|e| e.to_string())?;
        let matches: Vec<_> = poses.iter().filter(|p| p.name == self.link).collect();
        if matches.len() != 1 || !matches[0].valid_rigid_transform() {
            return Err("speed task requires one finite rigid body pose".into());
        }
        let pose = matches[0];
        let mut clearance = f64::INFINITY;
        for p in &self.hull {
            let world: [f64; 3] = std::array::from_fn(|i| pose.position_m[i]
                + (0..3).map(|j| pose.rotation[i][j] * p[j]).sum::<f64>());
            let gap = world[2] - art.floor_height(world[0], world[1]);
            if !gap.is_finite() { return Err("nonfinite speed-task hull clearance".into()); }
            clearance = clearance.min(gap);
        }
        let mut contact = false;
        for c in frame["contacts"].as_array().ok_or("missing speed-task contacts")? {
            let index = c["link"].as_u64().ok_or("invalid contact link")? as usize;
            if index == self.index && c.get("other").ok_or("missing contact other")?.is_null() {
                let force = c["force_n"][2].as_f64().filter(|x| x.is_finite()).ok_or("invalid contact force")?;
                contact |= force > 0.0;
            }
        }
        let position = [pose.position_m[0], pose.position_m[1]];
        let origin = self.origin.unwrap_or(position);
        let endpoint = net_displacement(origin, position)?;
        let displacement = endpoint.displacement_xy_m;
        let distance = endpoint.distance_m;
        let reward = distance - self.previous_distance_m;
        let speed = distance / self.horizon_s;
        if !distance.is_finite() || !reward.is_finite() || !speed.is_finite() {
            return Err("nonfinite speed-task progress".into());
        }
        self.origin = Some(origin);
        self.previous_distance_m = distance;
        let up = pose.rotation[2][2];
        Ok(SpeedObservation { displacement_xy_m: displacement, net_distance_m: distance,
            net_speed_m_s: speed, progress_reward_m: reward, body_up_z: up,
            hull_floor_clearance_m: clearance, body_floor_contact: contact,
            fallen: up <= 0.0 || clearance <= 0.0 || contact })
    }
}

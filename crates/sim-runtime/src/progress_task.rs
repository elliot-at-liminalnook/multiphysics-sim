//! Shape-independent task progress. Termination is configured by the environment.
use crate::session::{LinkPose, Scene};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use sim_domain_control::displacement::DisplacementAxes;
use sim_domain_control::displacement::measure;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressTaskConfig {
    /// Origin of an explicitly named CAD link, measured in the world frame.
    pub link: String,
    /// Explicit world axes; no gravity direction or ground plane is inferred.
    pub axes: DisplacementAxes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProgressObservation {
    pub displacement_world_m: [f64; 3],
    pub net_distance_m: f64,
    /// Diagnostic distance divided by the full requested episode duration.
    /// A terminated/unfinished episode is not an eligible completed speed result.
    pub net_speed_m_s: f64,
    pub progress_reward_m: f64,
}

pub(crate) struct ProgressMonitor {
    config: ProgressTaskConfig,
    horizon_s: f64,
    origin: Option<[f64; 3]>,
    previous_distance_m: f64,
}
impl ProgressMonitor {
    pub fn new(config: &ProgressTaskConfig, scene: &Scene, horizon_s: f64) -> Result<Self, String> {
        if scene
            .robot
            .links
            .iter()
            .filter(|l| l.name == config.link)
            .count()
            != 1
            || config.link.trim().is_empty()
            || !horizon_s.is_finite()
            || horizon_s <= 0.
        {
            return Err(
                "progress requires one named CAD link and a positive finite horizon".into(),
            );
        }
        Ok(Self {
            config: config.clone(),
            horizon_s,
            origin: None,
            previous_distance_m: 0.,
        })
    }
    pub fn observe(&mut self, frame: &Value) -> Result<ProgressObservation, String> {
        let poses: Vec<LinkPose> =
            serde_json::from_value(frame["poses"].clone()).map_err(|e| e.to_string())?;
        let matches: Vec<_> = poses
            .iter()
            .filter(|p| p.name == self.config.link)
            .collect();
        if matches.len() != 1 || !matches[0].valid_rigid_transform() {
            return Err("progress requires one finite rigid pose for its CAD link".into());
        }
        let position = matches[0].position_m;
        let origin = self.origin.unwrap_or(position);
        let s = measure(origin, position, self.config.axes)?;
        let progress_reward_m = s.distance_m - self.previous_distance_m;
        let net_speed_m_s = s.distance_m / self.horizon_s;
        if !progress_reward_m.is_finite() || !net_speed_m_s.is_finite() {
            return Err("nonfinite task progress".into());
        }
        self.origin = Some(origin);
        self.previous_distance_m = s.distance_m;
        Ok(ProgressObservation {
            displacement_world_m: s.displacement_m,
            net_distance_m: s.distance_m,
            net_speed_m_s,
            progress_reward_m,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn progress_allows_ground_contact_rotation_and_other_gravity_frames() {
        let scene: Scene = serde_json::from_value(
            json!({"version":1,"period_s":0.02,"duration_s":10.,"options":{"contact":false},
            "robot":{"gravity":[0.,0.,0.],"links":[{"name":"roller"}]}}),
        )
        .unwrap();
        let mut m = ProgressMonitor::new(
            &ProgressTaskConfig {
                link: "roller".into(),
                axes: DisplacementAxes::Xz,
            },
            &scene,
            10.,
        )
        .unwrap();
        let frame = |x, z| {
            json!({"poses":[{"name":"roller","position_m":[x,0.,z],"rotation":[[1.,0.,0.],[0.,-1.,0.],[0.,0.,-1.]]}],
            "contacts":[{"link":0,"other":null,"force_n":[0.,0.,10.]}]})
        };
        let mut total = 0.;
        for (x, z) in [(0., 0.), (3., 4.), (0., 4.), (0., 0.)] {
            let s = m.observe(&frame(x, z)).unwrap();
            total += s.progress_reward_m;
            assert_eq!(total, s.net_distance_m);
            assert_eq!(s.net_speed_m_s, s.net_distance_m / 10.);
        }
        assert_eq!(total, 0.);
    }
}

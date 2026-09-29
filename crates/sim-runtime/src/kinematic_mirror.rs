//! Display pose of a CAD robot from measured actuator coordinates, with the base
//! held still (a suspended fixture). Geometry only: the shared closure solver
//! places every link, and no forces, contact or motor dynamics are evaluated.
use crate::session::{LinkPose, Scene};
use serde::Serialize;
use sim_domain_robot::{
    Articulated, Generalized,
    articulated::{Options, embedding::RigidEmbedding},
};
use sim_core::Behavior;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct MirrorCoordinate {
    pub name: String,
    /// The CAD joint the motor drives.
    pub joint: String,
    /// CAD home value (radians for revolute, metres for prismatic DOFs).
    pub home: f64,
    pub lower: Option<f64>,
    pub upper: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct MirrorPose {
    pub coordinates: Vec<f64>,
    pub poses: Vec<LinkPose>,
    pub authored_limit_violations: Vec<String>,
    pub maximum_scaled_closure_error: f64,
}

pub struct KinematicMirror {
    art: Articulated,
    independent: Vec<String>,
    home: Vec<f64>,
    /// Last solved configuration; continuation keeps the assembly branch.
    seed: Generalized,
    lift_m: f64,
}

impl KinematicMirror {
    /// Every motor's articulated coordinate is independent, in CAD motor order;
    /// the base is held at its initial pose raised by `lift_m`.
    pub fn new(scene: Scene, lift_m: f64) -> Result<Self, String> {
        if !lift_m.is_finite() || !(0.0..=2.0).contains(&lift_m) {
            return Err("suspension lift must be 0..2 m".into());
        }
        let options = Options { planar: scene.options.planar, contact: false, omit_inter_link_contact: true, ..Options::default() };
        let art = Articulated::new(Arc::new(scene.robot), &options)?;
        let independent = art
            .model
            .motors
            .iter()
            .map(|m| {
                let joint = m.joint.as_ref().ok_or("motor has no declared joint")?;
                let found: Vec<_> = art.dofs().filter(|(j, _)| &j.name == joint).map(|(_, d)| d.name.clone()).collect();
                match found.as_slice() {
                    [name] => Ok(name.clone()),
                    _ => Err(format!("motor joint {joint} does not select one articulated coordinate")),
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        let seed = art.generalized(
            art.states().iter().map(|s| s.initial).collect(),
            vec![0.0; art.state_count],
            &vec![0.0; art.port_names.len() + 1],
            vec![],
        );
        let map = RigidEmbedding::new(&art, &independent, Default::default())?;
        let home = map.independent_joint_indices().iter().map(|&i| seed.q[i]).collect();
        let mut mirror = Self { art, independent, home, seed, lift_m };
        let home = mirror.home.clone();
        mirror.pose(&home)?;
        Ok(mirror)
    }

    /// CAD joint name of each coordinate, in motor order.
    pub fn motor_joints(&self) -> Vec<String> {
        self.art.model.motors.iter().filter_map(|m| m.joint.clone()).collect()
    }

    pub fn coordinates(&self) -> Vec<MirrorCoordinate> {
        let dofs: Vec<_> = self.art.dofs().map(|(_, d)| d).collect();
        self.independent
            .iter()
            .zip(&self.home)
            .zip(self.motor_joints())
            .map(|((name, &home), joint)| {
                let d = dofs.iter().find(|d| &d.name == name).expect("validated by embedding");
                MirrorCoordinate { name: name.clone(), joint, home, lower: d.lower, upper: d.upper }
            })
            .collect()
    }

    /// Inter-link penetrations (link names and depth, m) at these independent
    /// coordinates, from the shared sampled collision audit. Empty = clear.
    pub fn interference(&mut self, coordinates: &[f64]) -> Result<Vec<(String, String, f64)>, String> {
        let pose = self.pose(coordinates)?;
        let hits = crate::contact_audit::sampled_inter_link_penetrations(&self.art, &pose.poses)?;
        Ok(hits.into_iter().map(|h| (self.art.links[h.link].name.clone(), self.art.links[h.other].name.clone(), h.penetration_m)).collect())
    }
    /// Solve every dependent link for these independent coordinates. On failure
    /// the previous pose and branch are kept, so the caller can show the error.
    /// Large moves are solved in steps of at most `MAX_STEP` per coordinate so
    /// a closed linkage stays on its assembly branch (one solve from CAD home
    /// to the knee's mid-travel lands on the other slider-crank branch).
    pub fn pose(&mut self, coordinates: &[f64]) -> Result<MirrorPose, String> {
        const MAX_STEP: f64 = 0.1;
        if coordinates.len() != self.independent.len() || coordinates.iter().any(|c| !c.is_finite()) {
            return Err(format!("mirror needs {} finite motor coordinates", self.independent.len()));
        }
        let map = RigidEmbedding::new(&self.art, &self.independent, Default::default())?;
        let velocities = vec![0.0; map.reduced_dimension()];
        let from: Vec<f64> = map.independent_joint_indices().iter().map(|&i| self.seed.q[i]).collect();
        let largest = from.iter().zip(coordinates).map(|(a, b)| (b - a).abs()).fold(0.0, f64::max);
        // A corrupt reading far off the branch must not stall the display thread.
        let steps = (largest / MAX_STEP).ceil().clamp(1., 1000.) as usize;
        let mut seed = self.seed.clone();
        let mut motion = None;
        for k in 1..=steps {
            let t = k as f64 / steps as f64;
            let target: Vec<f64> = if k == steps { coordinates.to_vec() } else { from.iter().zip(coordinates).map(|(a, b)| a + (b - a) * t).collect() };
            let m = map.solve(&seed, &target, &velocities)?;
            seed = m.generalized.clone();
            motion = Some(m);
        }
        let motion = motion.expect("at least one step");
        let g = motion.generalized;
        let authored_limit_violations = self
            .art
            .dofs()
            .enumerate()
            .filter(|(i, (_, d))| d.lower.is_some_and(|lo| g.q[*i] < lo) || d.upper.is_some_and(|hi| g.q[*i] > hi))
            .map(|(_, (_, d))| d.name.clone())
            .collect();
        let poses = self
            .art
            .poses(&g)
            .iter()
            .zip(&self.art.links)
            .map(|((r, p), l)| LinkPose {
                name: l.name.clone(),
                position_m: [p.x, p.y, p.z + self.lift_m],
                rotation: std::array::from_fn(|i| std::array::from_fn(|j| r[(i, j)])),
            })
            .collect();
        self.seed = g;
        Ok(MirrorPose {
            coordinates: coordinates.to_vec(),
            poses,
            authored_limit_violations,
            maximum_scaled_closure_error: motion.maximum_scaled_position_error,
        })
    }
}

//! Readable joint-space pose sequences (stand, crouch, leg lifts): named poses
//! in degrees per leg and joint, played as timed moves and holds that loop
//! back to the first pose. Compiles to a rest-to-rest joint trajectory for
//! gait playback. No robot topology, kinematics or physics lives here; hosts
//! supply the joint list, the base pose and the feasibility checks.
use crate::trajectory::{Interpolation, Keyframe, Trajectory, TrajectoryConfig};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoseScript {
    pub version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Angles a pose does not name: the robot's standing pose or CAD home (0).
    #[serde(default)]
    pub base: Base,
    pub poses: BTreeMap<String, Pose>,
    /// Starts at the first step's pose; each later step moves to its pose over
    /// `move_s` and holds it `hold_s`; the first step's `move_s` returns to the
    /// start, so the sequence loops.
    pub sequence: Vec<Step>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Base {
    #[default]
    Stance,
    Home,
}

/// Joint angles in degrees, keyed like `foot_servo_deg`. `all` applies to
/// every leg with that joint; `legs` entries win over `all`; `extends` starts
/// from another pose instead of the base.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub all: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub legs: BTreeMap<String, BTreeMap<String, f64>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub pose: String,
    pub move_s: f64,
    #[serde(default)]
    pub hold_s: f64,
}

/// One robot coordinate as a pose file names it.
#[derive(Clone, Debug, PartialEq)]
pub struct JointRef {
    /// Host coordinate name, e.g. `joint.+X | Foot servo output`.
    pub coordinate: String,
    pub leg: String,
    /// e.g. `foot_servo_deg`.
    pub key: String,
}

/// Name coordinates `[joint.]LEG | Role output` as leg `LEG`, key `role_deg`.
pub fn joint_refs(coordinates: &[String]) -> Result<Vec<JointRef>, String> {
    let mut seen = BTreeSet::new();
    coordinates
        .iter()
        .map(|c| {
            let (leg, role) = c.trim_start_matches("joint.").split_once(" | ").ok_or(format!("coordinate {c} is not `LEG | Role`"))?;
            let key = format!("{}_deg", role.trim().trim_end_matches(" output").to_lowercase().replace(' ', "_"));
            if !seen.insert((leg.to_string(), key.clone())) {
                return Err(format!("two coordinates are both {leg}.{key}"));
            }
            Ok(JointRef { coordinate: c.clone(), leg: leg.trim().to_string(), key })
        })
        .collect()
}

/// A pose sequence as a joint trajectory, with each pose's angles (rad).
#[derive(Clone, Debug)]
pub struct Compiled {
    pub trajectory: TrajectoryConfig,
    pub poses: BTreeMap<String, Vec<f64>>,
    pub period_s: f64,
    /// Largest joint speed of the trajectory (rad/s), per coordinate.
    pub maximum_rates_rad_s: Vec<f64>,
}

impl PoseScript {
    /// A pose's angles (rad) in `joints` order. `base` is the host's pose for
    /// [`Base::Stance`]; [`Base::Home`] uses zeros.
    pub fn resolve(&self, name: &str, joints: &[JointRef], stance: &[f64]) -> Result<Vec<f64>, String> {
        if stance.len() != joints.len() {
            return Err("stance pose and joint list differ in length".into());
        }
        let mut chain = vec![];
        let mut at = name;
        loop {
            let pose = self.poses.get(at).ok_or(format!("unknown pose {at} (poses are {:?})", self.poses.keys().collect::<Vec<_>>()))?;
            if chain.iter().any(|(n, _)| *n == at) {
                return Err(format!("poses extend each other in a loop at {at}"));
            }
            chain.push((at, pose));
            match &pose.extends {
                Some(next) => at = next,
                None => break,
            }
        }
        let mut angles = match self.base {
            Base::Stance => stance.to_vec(),
            Base::Home => vec![0.; joints.len()],
        };
        for (pname, pose) in chain.into_iter().rev() {
            for (key, deg) in &pose.all {
                let mut hit = false;
                for (j, a) in joints.iter().zip(angles.iter_mut()) {
                    if &j.key == key {
                        *a = finite(*deg, pname, key)?.to_radians();
                        hit = true;
                    }
                }
                if !hit {
                    return Err(format!("poses.{pname}.all.{key}: no leg has this joint (joints are {:?})", keys(joints)));
                }
            }
            for (leg, values) in &pose.legs {
                if !joints.iter().any(|j| &j.leg == leg) {
                    return Err(format!("poses.{pname}.legs.{leg}: unknown leg (legs are {:?})", joints.iter().map(|j| &j.leg).collect::<BTreeSet<_>>()));
                }
                for (key, deg) in values {
                    let i = joints.iter().position(|j| &j.leg == leg && &j.key == key).ok_or(format!("poses.{pname}.legs.{leg}.{key}: unknown joint (joints are {:?})", keys(joints)))?;
                    angles[i] = finite(*deg, pname, key)?.to_radians();
                }
            }
        }
        Ok(angles)
    }

    pub fn compile(&self, joints: &[JointRef], stance: &[f64]) -> Result<Compiled, String> {
        if self.version != VERSION {
            return Err(format!("pose file version {} is not supported (expected {VERSION})", self.version));
        }
        let first = self.sequence.first().ok_or("sequence needs at least one step")?;
        for (i, s) in self.sequence.iter().enumerate() {
            if !(s.move_s.is_finite() && s.move_s > 0.) || !(s.hold_s.is_finite() && s.hold_s >= 0.) {
                return Err(format!("sequence[{i}]: move_s must be positive and hold_s zero or more"));
            }
        }
        let mut poses = BTreeMap::new();
        for name in self.poses.keys() {
            poses.insert(name.clone(), self.resolve(name, joints, stance)?);
        }
        let angles = |name: &str| poses.get(name).cloned().ok_or(format!("sequence names unknown pose {name}"));
        let mut t = 0.;
        let mut keyframes = vec![Keyframe { time_s: 0., values: angles(&first.pose)? }];
        let hold = |t: &mut f64, values: Vec<f64>, hold_s: f64, keyframes: &mut Vec<Keyframe>| {
            if hold_s > 0. {
                *t += hold_s;
                keyframes.push(Keyframe { time_s: *t, values });
            }
        };
        hold(&mut t, angles(&first.pose)?, first.hold_s, &mut keyframes);
        for s in &self.sequence[1..] {
            t += s.move_s;
            keyframes.push(Keyframe { time_s: t, values: angles(&s.pose)? });
            hold(&mut t, angles(&s.pose)?, s.hold_s, &mut keyframes);
        }
        t += first.move_s;
        keyframes.push(Keyframe { time_s: t, values: angles(&first.pose)? });
        let trajectory = TrajectoryConfig { interpolation: Interpolation::QuinticRestToRest, keyframes };
        let rates = Trajectory::new(trajectory.clone())?.maximum_absolute_rates()?;
        Ok(Compiled { trajectory, poses, period_s: t, maximum_rates_rad_s: rates })
    }
}

fn finite(v: f64, pose: &str, key: &str) -> Result<f64, String> {
    if v.is_finite() { Ok(v) } else { Err(format!("poses.{pose}: {key} must be a finite angle")) }
}

fn keys(joints: &[JointRef]) -> BTreeSet<&str> {
    joints.iter().map(|j| j.key.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joints() -> Vec<JointRef> {
        let names: Vec<String> = ["-Y", "+X"].iter().flat_map(|l| ["Hip", "Worm", "Foot"].map(|r| format!("joint.{l} | {r} servo output"))).collect();
        joint_refs(&names).unwrap()
    }
    const STANCE: [f64; 6] = [0., 0., -1.0472, 0., 0., -1.0472];

    fn script() -> PoseScript {
        serde_json::from_value(serde_json::json!({
            "version": 1, "name": "crouch",
            "poses": {
                "stand": {},
                "crouch": {"all": {"foot_servo_deg": -100}},
                "lift": {"extends": "crouch", "legs": {"+X": {"hip_servo_deg": 20}}}
            },
            "sequence": [
                {"pose": "stand", "move_s": 1.0, "hold_s": 0.5},
                {"pose": "crouch", "move_s": 2.0},
                {"pose": "lift", "move_s": 0.8, "hold_s": 1.0}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn coordinates_become_leg_and_joint_keys() {
        let j = joints();
        assert_eq!((j[2].leg.as_str(), j[2].key.as_str()), ("-Y", "foot_servo_deg"));
        assert!(joint_refs(&["joint.+X Hip".into()]).is_err());
    }

    #[test]
    fn poses_layer_base_extends_all_and_legs() {
        let s = script();
        let lift = s.resolve("lift", &joints(), &STANCE).unwrap();
        assert_eq!(lift[0], 0.);
        assert!((lift[2] - (-100f64).to_radians()).abs() < 1e-15, "crouch applies to every leg");
        assert!((lift[3] - 20f64.to_radians()).abs() < 1e-15, "the leg entry wins");
        assert_eq!(s.resolve("stand", &joints(), &STANCE).unwrap(), STANCE, "an empty pose is the stance");
        let mut home = s.clone();
        home.base = Base::Home;
        assert_eq!(home.resolve("stand", &joints(), &STANCE).unwrap(), vec![0.; 6]);
    }

    #[test]
    fn sequence_times_moves_and_holds_and_loops_back() {
        let c = script().compile(&joints(), &STANCE).unwrap();
        let times: Vec<f64> = c.trajectory.keyframes.iter().map(|k| k.time_s).collect();
        assert_eq!(times, [0., 0.5, 2.5, 3.3, 4.3, 5.3]);
        assert_eq!(c.trajectory.keyframes[0].values, c.trajectory.keyframes.last().unwrap().values, "loops");
        assert_eq!(c.period_s, 5.3);
        // Rest-to-rest quintic: peak speed 1.875 x distance / time.
        let crouch = ((-100f64).to_radians() + 1.0472).abs();
        assert!((c.maximum_rates_rad_s[2] - 1.875 * crouch / 1.0).abs() < 1e-6, "return from crouch over 1 s is the fastest foot move");
    }

    #[test]
    fn mistakes_are_reported_by_path() {
        let err = |f: fn(&mut PoseScript)| {
            let mut s = script();
            f(&mut s);
            s.compile(&joints(), &STANCE).unwrap_err()
        };
        assert!(err(|s| s.sequence[1].pose = "sit".into()).contains("unknown pose sit"));
        assert!(err(|s| { s.poses.get_mut("crouch").unwrap().legs.insert("+Z".into(), BTreeMap::new()); }).contains("unknown leg"));
        assert!(err(|s| { s.poses.get_mut("crouch").unwrap().all.insert("knee_deg".into(), 1.); }).contains("no leg has this joint"));
        assert!(err(|s| s.poses.get_mut("crouch").unwrap().extends = Some("lift".into())).contains("loop"));
        assert!(err(|s| s.sequence[1].move_s = 0.).contains("move_s must be positive"));
        assert!(err(|s| s.sequence.clear()).contains("at least one step"));
    }
}

//! The drive twist steering a legged gait: [`SteeredGait`] already takes a
//! body twist `[forward m/s, lateral m/s, yaw rad/s]` in the same frame and
//! sign conventions as [`BodyTwist`] (lateral +left, yaw +counter-clockwise),
//! so device bindings, drive profiles and the shared limiter serve walking
//! robots unchanged; only the kinematic adapter differs (a gait instead of
//! a wheel mixer). `SteeredGait` clamps to its own steering bounds and rate
//! limits on top of whatever the drive limiter already applied.
use super::kinematics::BodyTwist;
use crate::contact_phase::steered::SteeredGait;

/// Request `twist` from `time_s` on (non-decreasing times), exactly
/// `gait.command(time_s, twist.to_array())`.
pub fn command_steered(gait: &mut SteeredGait, time_s: f64, twist: BodyTwist) -> Result<(), String> {
    gait.command(time_s, twist.to_array())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_phase::steered::{PathStart, SteeringConfig};
    use crate::contact_phase::{ContactPhaseConfig, FootPhase, FootStep};
    use crate::trajectory::{Interpolation, Keyframe, TrajectoryConfig};

    /// The gait of contact_phase::steered's own tests: four plus-layout
    /// feet, one with two steps per cycle, and a body that sways and bobs.
    fn gait() -> ContactPhaseConfig {
        let foot = |center: [f64; 3], phase: f64| FootPhase {
            center_world_m: center,
            phase_offset: phase,
            stance_fraction: 0.56,
            swing_offset_world_m: [0., 0.004, 0.012],
            return_ramp_fraction: Some(0.25),
            additional_steps: vec![],
        };
        let mut feet = vec![foot([0., -0.2, -0.1], 0.), foot([0.2, 0., -0.1], 0.546), foot([0., 0.2, -0.1], 0.066), foot([-0.2, 0., -0.1], 0.575)];
        feet[3].stance_fraction = 0.3;
        feet[3].additional_steps = vec![FootStep {
            center_world_m: [-0.21, 0.01, -0.1],
            phase_offset: 0.1,
            stance_fraction: 0.3,
            swing_offset_world_m: [0.002, 0., 0.01],
            return_ramp_fraction: None,
        }];
        let period = 1.42;
        ContactPhaseConfig {
            period_s: period,
            displacement_world_m: [0.27, 0.27, 0.],
            body: TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes: (0..=4)
                    .map(|i| Keyframe {
                        time_s: period * i as f64 / 4.,
                        values: [[0.01, 0., 0.13, 0., 0.02, 0.01], [0., 0.01, 0.12, 0.01, 0., 0.], [-0.01, 0., 0.13, 0., -0.02, -0.01], [0., -0.01, 0.12, -0.01, 0., 0.]][i % 4].to_vec(),
                    })
                    .collect(),
            },
            feet,
        }
    }

    fn steering() -> SteeringConfig {
        SteeringConfig { maximum_twist: [0.3, 0.3, 0.5], maximum_twist_rate: [0.2, 0.2, 0.5], plan_step_s: 0.01, forward_axis_rad: 0. }
    }

    #[test]
    fn a_body_twist_steers_the_gait_like_its_array() {
        let twist = BodyTwist::new(0.2, -0.05, 0.3);
        let mut through_drive = SteeredGait::new(gait(), steering(), [0.02, -0.01], PathStart::default()).unwrap();
        let mut direct = SteeredGait::new(gait(), steering(), [0.02, -0.01], PathStart::default()).unwrap();
        for k in 0..=150 {
            let t = k as f64 * 0.02;
            command_steered(&mut through_drive, t, twist).unwrap();
            direct.command(t, [0.2, -0.05, 0.3]).unwrap();
        }
        let (a, b) = (through_drive.sample(3.0).unwrap(), direct.sample(3.0).unwrap());
        assert_eq!(a.path_pose, b.path_pose);
        assert_eq!(a.path_twist, b.path_twist);
        // Past the horizon and the rate-limited ramp, the path runs at the twist.
        assert!(3.0 > through_drive.horizon_s() + 1.0);
        for (got, want) in a.path_twist.iter().zip(twist.to_array()) {
            assert!((got - want).abs() < 1e-12, "path twist {:?} != {:?}", a.path_twist, twist.to_array());
        }
        // A non-finite twist is refused, as SteeredGait refuses it.
        assert!(command_steered(&mut through_drive, 3.1, BodyTwist::new(f64::NAN, 0., 0.)).is_err());
    }
}

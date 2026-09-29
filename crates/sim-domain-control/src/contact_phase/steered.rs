//! A contact-phase gait steered by a body-frame twist command (forward,
//! lateral, yaw rate). The gait keeps its timing, body oscillation, swing
//! shape and footholds relative to the body; only the body path changes.
//!
//! The planned path is fixed one commitment horizon ahead: a command at time
//! t shapes the path after t + horizon, rate limited per component. Every
//! foothold lies at the gait's center, carried by the path pose at that
//! step's mid-stance, and is final before its foot lifts off, so stance feet
//! never move and swings end exactly on their footholds. A constant command
//! equal to [`native_twist`] reproduces [`ContactPhaseMotion`]. Geometric
//! reference only: balance, contact and actuator limits are checked where
//! the reference is executed.
use super::{ContactPhaseConfig, ContactPhaseMotion, FootSample, Placement};
use crate::planar::{advance_planar, rotate, to_world, yaw_rotation_vector};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SteeringConfig {
    /// Symmetric command bounds, body frame: forward and lateral speed (m/s),
    /// yaw rate (rad/s). Larger commands are clamped.
    pub maximum_twist: [f64; 3],
    /// Largest change per second of each component (m/s², m/s², rad/s²).
    pub maximum_twist_rate: [f64; 3],
    /// Spacing of the planned path (s); the twist is constant between knots.
    pub plan_step_s: f64,
    /// Direction of the command's forward axis in the gait's frame (rad
    /// about +Z), e.g. the direction a gait was designed to walk. Commands,
    /// their bounds and rate limits are in this command frame.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub forward_axis_rad: f64,
}
fn is_zero(v: &f64) -> bool {
    *v == 0.
}

/// The constant command that reproduces a periodic gait's own travel: its
/// horizontal displacement per period, in the command frame, no turn.
pub fn native_twist(gait: &ContactPhaseConfig, steering: &SteeringConfig) -> [f64; 3] {
    let v = rotate(
        -steering.forward_axis_rad,
        [gait.displacement_world_m[0] / gait.period_s, gait.displacement_world_m[1] / gait.period_s],
    );
    [v[0], v[1], 0.]
}

/// Where the planned path starts: time (s), pose (x, y m and heading rad of
/// the gait frame) and command-frame twist (within the steering bounds).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PathStart {
    pub time_s: f64,
    pub pose: [f64; 3],
    pub twist: [f64; 3],
}

#[derive(Clone, Copy, Debug)]
struct Knot {
    pose: [f64; 3],
    twist: [f64; 3],
}

#[derive(Clone, Debug, Serialize)]
pub struct SteeredSample {
    /// Planned body path: world x, y (m) and heading (rad).
    pub path_pose: [f64; 3],
    /// Path twist in the command frame: forward, lateral (m/s), yaw rate (rad/s).
    pub path_twist: [f64; 3],
    /// Base offset from its starting position, world axes, with its rates
    /// (the same quantity as [`super::ContactPhaseSample::body`] values 0..3).
    pub body_offset_m: [f64; 3],
    pub body_velocity_m_s: [f64; 3],
    pub body_acceleration_m_s2: [f64; 3],
    /// The gait's periodic body rotation turned with the heading, world axes,
    /// applied before the base's starting rotation.
    pub body_rotation_vector_rad: [f64; 3],
    pub feet: Vec<FootSample>,
}

pub struct SteeredGait {
    motion: ContactPhaseMotion,
    config: SteeringConfig,
    origin: [f64; 2],
    horizon_s: f64,
    start_s: f64,
    start: Knot,
    /// Index of `knots[0]`; knot k sits at start_s + k * plan_step_s.
    first: u64,
    knots: VecDeque<Knot>,
    command: [f64; 3],
    commanded_s: f64,
}

impl SteeredGait {
    /// `origin_world_m` is the base's starting horizontal position: the point
    /// the gait's body offsets are measured from and the path turns about.
    /// Times before `start.time_s` continue the starting twist backwards, so
    /// a gait can begin mid-cycle with its earlier footholds consistent.
    pub fn new(
        gait: ContactPhaseConfig,
        config: SteeringConfig,
        origin_world_m: [f64; 2],
        start: PathStart,
    ) -> Result<Self, String> {
        let PathStart { time_s: start_time_s, pose: start_pose, twist: start_twist } = start;
        let motion = ContactPhaseMotion::new(gait)?;
        if config.maximum_twist.iter().chain(&config.maximum_twist_rate).any(|v| !v.is_finite() || *v < 0.)
            || config.maximum_twist_rate.iter().any(|v| *v == 0.)
            || !config.plan_step_s.is_finite()
            || config.plan_step_s <= 0.
            || config.plan_step_s > motion.config.period_s
            || !config.forward_axis_rad.is_finite()
        {
            return Err("steering needs finite nonnegative twist bounds, positive twist rates and a plan step within one period".into());
        }
        if !start_time_s.is_finite()
            || origin_world_m.iter().any(|v| !v.is_finite())
            || start_pose.iter().any(|v| !v.is_finite())
            || (0..3).any(|i| !start_twist[i].is_finite() || start_twist[i].abs() > config.maximum_twist[i])
        {
            return Err("finite start time and pose, and a start twist within maximum_twist, required".into());
        }
        let horizon_s = motion.sequences.iter().map(|s| s.commitment()).fold(0., f64::max) * motion.config.period_s;
        let start = Knot { pose: start_pose, twist: start_twist };
        let mut gait = Self {
            motion,
            config,
            origin: origin_world_m,
            horizon_s,
            start_s: start_time_s,
            start,
            first: 0,
            knots: VecDeque::from([start]),
            command: start_twist,
            commanded_s: start_time_s,
        };
        gait.extend(start_time_s + horizon_s + gait.config.plan_step_s);
        Ok(gait)
    }

    pub fn config(&self) -> &SteeringConfig {
        &self.config
    }
    pub fn gait(&self) -> &ContactPhaseConfig {
        &self.motion.config
    }
    /// How far ahead the path is fixed (s): the delay before a command starts
    /// to change the path.
    pub fn horizon_s(&self) -> f64 {
        self.horizon_s
    }

    /// A point given at heading zero with the path at its origin (world
    /// coordinates, like the gait's foot centers), carried by path `pose`.
    pub fn carry(&self, pose: [f64; 3], point_world_m: [f64; 3]) -> [f64; 3] {
        let o = self.origin;
        let p = to_world(pose, [point_world_m[0] - o[0], point_world_m[1] - o[1], point_world_m[2]]);
        [p[0] + o[0], p[1] + o[1], p[2]]
    }

    fn knot_time(&self, index: u64) -> f64 {
        self.start_s + index as f64 * self.config.plan_step_s
    }

    fn extend(&mut self, until_s: f64) {
        let h = self.config.plan_step_s;
        while self.knot_time(self.first + self.knots.len() as u64 - 1) < until_s {
            let last = *self.knots.back().expect("the plan keeps at least one knot");
            let twist = std::array::from_fn(|i| {
                let limit = self.config.maximum_twist_rate[i] * h;
                last.twist[i] + (self.command[i] - last.twist[i]).clamp(-limit, limit)
            });
            self.knots.push_back(Knot { pose: advance_planar(last.pose, self.gait_twist(last.twist), h), twist });
        }
    }

    /// Request a body twist from `time_s` on (non-decreasing times). It
    /// shapes the path after `time_s + horizon_s()`.
    pub fn command(&mut self, time_s: f64, twist: [f64; 3]) -> Result<(), String> {
        if !time_s.is_finite() || time_s < self.commanded_s || twist.iter().any(|v| !v.is_finite()) {
            return Err("finite twist and non-decreasing command time required".into());
        }
        self.command = std::array::from_fn(|i| twist[i].clamp(-self.config.maximum_twist[i], self.config.maximum_twist[i]));
        self.commanded_s = time_s;
        // One extra knot absorbs rounding in mid-stance times at the horizon.
        self.extend(time_s + self.horizon_s + self.config.plan_step_s);
        // Keep what a swing or stance at `time_s` can still refer back to.
        let keep = time_s - self.motion.config.period_s - self.config.plan_step_s;
        while self.knots.len() > 1 && self.knot_time(self.first + 1) < keep {
            self.knots.pop_front();
            self.first += 1;
        }
        Ok(())
    }

    /// A command-frame twist in the gait's body frame.
    fn gait_twist(&self, twist: [f64; 3]) -> [f64; 3] {
        let v = rotate(self.config.forward_axis_rad, [twist[0], twist[1]]);
        [v[0], v[1], twist[2]]
    }

    fn path(&self, time_s: f64) -> Result<Knot, String> {
        let h = self.config.plan_step_s;
        if time_s < self.start_s {
            return Ok(Knot { pose: advance_planar(self.start.pose, self.gait_twist(self.start.twist), time_s - self.start_s), twist: self.start.twist });
        }
        let index = ((time_s - self.start_s) / h).floor() as u64;
        let last = self.first + self.knots.len() as u64 - 1;
        if index < self.first || index > last || (index == last && time_s > self.knot_time(last)) {
            return Err(format!(
                "steered path is planned for {:.4}..{:.4} s, not {time_s:.4} s; command the gait up to {:.4} s first",
                self.knot_time(self.first),
                self.knot_time(last),
                time_s - self.horizon_s
            ));
        }
        let knot = self.knots[(index - self.first) as usize];
        Ok(Knot { pose: advance_planar(knot.pose, self.gait_twist(knot.twist), time_s - self.knot_time(index)), twist: knot.twist })
    }

    /// Reference at `time_s`; the path must already be planned through its
    /// footholds (any time up to the latest command time).
    pub fn sample(&self, time_s: f64) -> Result<SteeredSample, String> {
        if !time_s.is_finite() {
            return Err("finite sample time required".into());
        }
        let p = self.motion.config.period_s;
        let cycles = time_s / p;
        let feet = self
            .motion
            .sequences
            .iter()
            .map(|sequence| {
                sequence.sample(cycles, p, |i, turn| {
                    // Placements come from the fixed plan; a failure here is
                    // reported below by the explicit horizon check instead.
                    let pose = self.path(sequence.midstance(i, turn) * p).map(|k| k.pose).unwrap_or([f64::NAN; 3]);
                    Placement { position_world_m: self.carry(pose, sequence.steps()[i].center_world_m), heading_rad: pose[2] }
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if feet.iter().flat_map(|f| f.position_world_m).any(|v| !v.is_finite()) {
            self.path(time_s + self.horizon_s)?;
            self.path(time_s - p)?;
            return Err("nonfinite steered foothold".into());
        }
        let path = self.path(time_s)?;
        let b = self.motion.body.sample(time_s.rem_euclid(p))?;
        let (pose, w) = (path.pose, self.gait_twist(path.twist));
        let turn = |v: [f64; 2]| rotate(pose[2], v);
        let cross = |v: [f64; 2]| [-w[2] * v[1], w[2] * v[0]];
        let travel = turn([w[0], w[1]]);
        // The base offset turns about the base's own start (the origin).
        let offset = turn([b.values[0], b.values[1]]);
        let rate = turn([b.rates[0], b.rates[1]]);
        let acceleration = turn([b.accelerations[0], b.accelerations[1]]);
        let (spin_offset, spin_rate, spin_travel) = (cross(offset), cross(rate), cross(travel));
        Ok(SteeredSample {
            path_pose: pose,
            path_twist: path.twist,
            body_offset_m: [pose[0] + offset[0], pose[1] + offset[1], b.values[2]],
            body_velocity_m_s: [
                travel[0] + rate[0] + spin_offset[0],
                travel[1] + rate[1] + spin_offset[1],
                b.rates[2],
            ],
            body_acceleration_m_s2: [
                spin_travel[0] + acceleration[0] + 2. * spin_rate[0] - w[2] * w[2] * offset[0],
                spin_travel[1] + acceleration[1] + 2. * spin_rate[1] - w[2] * w[2] * offset[1],
                b.accelerations[2],
            ],
            body_rotation_vector_rad: yaw_rotation_vector(pose[2], [b.values[3], b.values[4], b.values[5]]),
            feet,
        })
    }

    /// Command then sample at one controller tick.
    pub fn step(&mut self, time_s: f64, twist: [f64; 3]) -> Result<SteeredSample, String> {
        self.command(time_s, twist)?;
        self.sample(time_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_phase::{FootPhase, FootStep};
    use crate::trajectory::{Interpolation, Keyframe, TrajectoryConfig};

    /// Four plus-layout feet, one with two steps per cycle, and a body that
    /// sways, bobs and pitches.
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
    fn native_command_reproduces_the_periodic_gait() {
        for axis in [0., std::f64::consts::FRAC_PI_4] {
            native_command_reproduces_the_periodic_gait_with(SteeringConfig { forward_axis_rad: axis, ..steering() });
        }
    }
    fn native_command_reproduces_the_periodic_gait_with(steering: SteeringConfig) {
        let config = gait();
        let periodic = ContactPhaseMotion::new(config.clone()).unwrap();
        let twist = native_twist(&config, &steering);
        if steering.forward_axis_rad != 0. {
            assert!(twist[1].abs() < 1e-15, "the gait walks along its forward axis");
        }
        let mut steered = SteeredGait::new(config, steering, [0.02, -0.01], PathStart { twist, ..Default::default() }).unwrap();
        for k in 0..800 {
            let t = k as f64 * 0.02 + 0.0013;
            let s = steered.step(t, twist).unwrap();
            let q = periodic.sample(t).unwrap();
            for i in 0..3 {
                assert!((s.body_offset_m[i] - q.body.values[i]).abs() < 1e-10, "body at {t}");
                assert!((s.body_velocity_m_s[i] - q.body.rates[i]).abs() < 1e-12);
                assert!((s.body_acceleration_m_s2[i] - q.body.accelerations[i]).abs() < 1e-12);
                assert!((s.body_rotation_vector_rad[i] - q.body.values[3 + i]).abs() < 1e-15);
            }
            for (a, b) in s.feet.iter().zip(&q.feet) {
                assert_eq!(a.in_contact, b.in_contact);
                assert!((a.phase - b.phase).abs() < 1e-12);
                for i in 0..3 {
                    assert!((a.position_world_m[i] - b.position_world_m[i]).abs() < 1e-10, "foot at {t}");
                    assert!((a.velocity_world_m_s[i] - b.velocity_world_m_s[i]).abs() < 1e-9);
                    assert!((a.acceleration_world_m_s2[i] - b.acceleration_world_m_s2[i]).abs() < 1e-7);
                }
            }
        }
    }

    /// Forward, arc, turn in place, sideways, backward, stop.
    fn commands(t: f64) -> [f64; 3] {
        match t {
            t if t < 3. => [0.2, 0., 0.],
            t if t < 6. => [0.15, 0., 0.3],
            t if t < 9. => [0., 0., -0.4],
            t if t < 12. => [0., 0.15, 0.],
            t if t < 15. => [-0.15, 0., 0.],
            _ => [0.; 3],
        }
    }

    #[test]
    fn commanded_sequence_keeps_stance_feet_planted_and_references_smooth() {
        let config = gait();
        let mut steered = SteeredGait::new(config.clone(), steering(), [0.02, -0.01], PathStart::default()).unwrap();
        let dt = 0.002;
        let mut previous: Option<SteeredSample> = None;
        let mut max_twist_change = [0f64; 3];
        for k in 0..10_000 {
            let t = k as f64 * dt;
            let s = steered.step(t, commands(t)).unwrap();
            if let Some(p) = &previous {
                for (a, b) in p.feet.iter().zip(&s.feet) {
                    for i in 0..3 {
                        // Positions follow the reported velocities (no jumps at events or command changes).
                        let predicted = a.position_world_m[i] + 0.5 * dt * (a.velocity_world_m_s[i] + b.velocity_world_m_s[i]);
                        assert!((b.position_world_m[i] - predicted).abs() < 2e-6, "foot jump at {t}");
                        if a.in_contact && b.in_contact && (b.phase > a.phase) {
                            assert_eq!(a.position_world_m[i], b.position_world_m[i], "stance foot moved at {t}");
                        }
                    }
                }
                for i in 0..3 {
                    let predicted = p.body_offset_m[i] + 0.5 * dt * (p.body_velocity_m_s[i] + s.body_velocity_m_s[i]);
                    assert!((s.body_offset_m[i] - predicted).abs() < 5e-6, "body jump at {t}");
                    max_twist_change[i] = max_twist_change[i].max((s.path_twist[i] - p.path_twist[i]).abs());
                }
            }
            previous = Some(s);
        }
        for i in 0..3 {
            assert!(max_twist_change[i] <= steering().maximum_twist_rate[i] * steering().plan_step_s + 1e-15);
        }
        // Stopped: the path holds still and the feet step in place.
        let a = steered.sample(19.99).unwrap();
        assert_eq!(a.path_twist, [0.; 3]);
        let b = steered.step(19.99 + config.period_s, [0.; 3]).unwrap();
        for (f, g) in a.feet.iter().zip(&b.feet) {
            for i in 0..3 {
                assert!((f.position_world_m[i] - g.position_world_m[i]).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn footholds_sit_at_the_gait_center_under_the_mid_stance_pose() {
        let config = gait();
        let p = config.period_s;
        let mut steered = SteeredGait::new(config, steering(), [0.02, -0.01], PathStart { pose: [0.3, -0.1, 0.7], ..Default::default() }).unwrap();
        // Visit every mid-stance of cycles 5..12 while commanding an arc.
        let mut visits = vec![];
        for (index, foot) in steered.motion.sequences.iter().enumerate() {
            for (i, step) in foot.steps().iter().enumerate() {
                for turn in 5..13 {
                    visits.push((foot.midstance(i, turn as f64) * p, index, step.center_world_m));
                }
            }
        }
        visits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut checked = 0;
        for (mid_s, index, center) in visits {
            let at_mid = steered.step(mid_s, [0.1, -0.05, 0.25]).unwrap();
            let expected = steered.carry(at_mid.path_pose, center);
            assert!(at_mid.feet[index].in_contact);
            for a in 0..3 {
                assert!((at_mid.feet[index].position_world_m[a] - expected[a]).abs() < 1e-12);
            }
            checked += 1;
        }
        assert_eq!(checked, 40);
        assert!(steered.sample(18.).unwrap().path_pose[2] > 3.);
    }

    #[test]
    fn a_turned_start_turns_body_and_feet_rigidly_about_the_base() {
        let config = gait();
        let origin = [0.02, -0.01];
        let yaw = 0.9;
        let mut straight = SteeredGait::new(config.clone(), steering(), origin, PathStart::default()).unwrap();
        let mut turned = SteeredGait::new(config, steering(), origin, PathStart { pose: [0., 0., yaw], ..Default::default() }).unwrap();
        for k in 0..600 {
            let t = k as f64 * 0.01;
            let command = [0.1, 0.05, 0.];
            let a = straight.step(t, command).unwrap();
            let b = turned.step(t, command).unwrap();
            let base = |s: &SteeredSample| [origin[0] + s.body_offset_m[0], origin[1] + s.body_offset_m[1], s.body_offset_m[2]];
            let (ba, bb) = (base(&a), base(&b));
            for (fa, fb) in a.feet.iter().zip(&b.feet) {
                let ra = [fa.position_world_m[0] - ba[0], fa.position_world_m[1] - ba[1]];
                let rb = [fb.position_world_m[0] - bb[0], fb.position_world_m[1] - bb[1]];
                let expected = rotate(yaw, ra);
                assert!((rb[0] - expected[0]).abs() < 1e-12 && (rb[1] - expected[1]).abs() < 1e-12, "at {t}");
                assert!((fa.position_world_m[2] - fb.position_world_m[2]).abs() < 1e-15);
            }
            let heading = yaw_rotation_vector(yaw, a.body_rotation_vector_rad);
            for i in 0..3 {
                assert!((heading[i] - b.body_rotation_vector_rad[i]).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn commands_wait_one_horizon_and_invalid_use_fails() {
        let config = gait();
        let mut steered = SteeredGait::new(config.clone(), steering(), [0.02, -0.01], PathStart::default()).unwrap();
        let horizon = steered.horizon_s();
        // Longest: -X foot's swing to its next step plus half that stance.
        assert!(horizon > 0.5 * config.period_s && horizon < config.period_s);
        steered.command(0.2, [0.2, 0., 0.]).unwrap();
        assert_eq!(steered.path(horizon - 0.02).unwrap().twist, [0.; 3]);
        assert!(steered.path(0.2 + horizon).unwrap().twist[0] > 0.);
        assert!(steered.sample(1.).is_err());
        assert!(steered.command(-1., [0.; 3]).is_err());
        steered.command(0.5, [9., -9., f64::MAX]).unwrap();
        steered.command(30., [0.; 3]).unwrap();
        assert!(steered.path(30. + horizon).unwrap().twist.iter().zip(&steering().maximum_twist).all(|(v, m)| v.abs() <= *m));
        assert!(steered.sample(1.).is_err(), "pruned history is reported, not extrapolated");
        let mut bad = steering();
        bad.maximum_twist_rate[2] = 0.;
        assert!(SteeredGait::new(config.clone(), bad, [0.; 2], PathStart::default()).is_err());
        assert!(SteeredGait::new(config, steering(), [0.; 2], PathStart { twist: [1., 0., 0.], ..Default::default() }).is_err());
    }
}

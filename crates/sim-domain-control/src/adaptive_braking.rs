//! Online, direction-specific stopping evidence and a measured-state speed envelope.
//! Estimates are conditional observations, never certified mechanical limits.
use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorRegistry, QuantityKind as Q,
    primitive::{Descriptor, Field as F},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub bootstrap_speed_rad_s: f64,
    pub fallback_deceleration_rad_s2: f64,
    pub reaction_time_s: f64,
    pub position_uncertainty_rad: f64,
    pub boundary_margin_rad: f64,
    pub learning_inset_fraction: f64,
    pub minimum_stops: usize,
    pub braking_safety_factor: f64,
    pub trial_speed_growth: f64,
    pub maximum_evidence_age_s: f64,
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.bootstrap_speed_rad_s,
            self.fallback_deceleration_rad_s2,
            self.reaction_time_s,
            self.position_uncertainty_rad,
            self.boundary_margin_rad,
            self.maximum_evidence_age_s,
        ]
        .iter()
        .any(|x| !x.is_finite() || *x <= 0.)
            || !(0.2..=0.4).contains(&self.learning_inset_fraction)
            || !(0.1..=0.5).contains(&self.braking_safety_factor)
            || !(1.01..=1.25).contains(&self.trial_speed_growth)
            || !(3..=16).contains(&self.minimum_stops)
        {
            return Err("Invalid adaptive braking assumptions".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StopEvidence {
    pub speed_rad_s: f64,
    pub excursion_rad: f64,
    pub effective_deceleration_rad_s2: f64,
    pub completed_s: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DirectionEvidence {
    pub stops: Vec<StopEvidence>,
    pub acceleration_rad_s2: f64,
    pub weakened_deceleration_rad_s2: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Episode {
    direction: usize,
    start_rad: f64,
    speed_rad_s: f64,
    peak_rad: f64,
    interior: bool,
    quiet_window: Option<(f64, f64, f64)>,
    predicted_rad: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Model {
    /// Index 0 is decreasing encoder angle, index 1 increasing. No inferred joint orientation.
    pub directions: [DirectionEvidence; 2],
    pub completed_stops: u64,
    pub reset_reason: Option<String>,
    episode: Option<Episode>,
    previous: Option<(f64, f64)>,
    context: Option<(f64, f64, f64)>,
    was_braking: bool,
    motion_speed_limit_rad_s: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub permitted_speed_rad_s: f64,
    pub stopping_distance_rad: f64,
    pub deceleration_rad_s2: f64,
    pub measured_speed_limit_rad_s: f64,
    pub accepted_stops: usize,
    pub brake_now: bool,
    pub margin_rad: f64,
}
impl Model {
    pub fn reset(&mut self, reason: &str) {
        *self = Self::default();
        self.reset_reason = Some(reason.into());
    }
    fn evidence<'a>(&'a self, c: &Config, d: usize, time: f64) -> Vec<&'a StopEvidence> {
        self.directions[d]
            .stops
            .iter()
            .filter(|s| time >= s.completed_s && time - s.completed_s <= c.maximum_evidence_age_s)
            .collect()
    }
    /// A noisy velocity reading or overshoot cannot authorize more speed than was requested.
    pub fn constrain_observed_speed(&mut self, limit: f64) -> Result<(), String> {
        if !limit.is_finite() || limit <= 0. {
            return Err("Invalid evidence speed ceiling".into());
        }
        self.motion_speed_limit_rad_s = Some(limit);
        Ok(())
    }
    pub fn measuring_stop(&self) -> bool {
        self.episode.is_some()
    }
    pub fn accepted_stops(&self, c: &Config, d: usize, time: f64) -> usize {
        self.evidence(c, d, time).len()
    }
    pub fn demonstrated_speed(&self, c: &Config, d: usize, time: f64) -> f64 {
        let mut speeds = self
            .evidence(c, d, time)
            .iter()
            .map(|s| s.speed_rad_s)
            .collect::<Vec<_>>();
        speeds.sort_by(|a, b| b.total_cmp(a));
        speeds
            .get(c.minimum_stops - 1)
            .copied()
            .unwrap_or(0.)
            .max(c.bootstrap_speed_rad_s)
    }
    pub fn deceleration(&self, c: &Config, d: usize, time: f64) -> f64 {
        let e = self.evidence(c, d, time);
        let measured = e
            .iter()
            .map(|s| s.effective_deceleration_rad_s2 * c.braking_safety_factor)
            .fold(f64::INFINITY, f64::min);
        let mut a = if e.len() >= c.minimum_stops {
            measured
        } else {
            measured.min(c.fallback_deceleration_rad_s2)
        };
        if let Some(weak) = self.directions[d].weakened_deceleration_rad_s2 {
            a = a.min(weak);
        }
        a.max(1e-9)
    }
    pub fn envelope(
        &self,
        c: &Config,
        time: f64,
        position: f64,
        velocity: f64,
        bounds: [f64; 2],
        direction: usize,
        requested: f64,
        exploring: bool,
    ) -> Result<Envelope, String> {
        c.validate()?;
        if [time, position, velocity, bounds[0], bounds[1], requested]
            .iter()
            .any(|x| !x.is_finite())
            || bounds[0] >= bounds[1]
            || direction > 1
            || requested < 0.
            || position < bounds[0]
            || position > bounds[1]
        {
            return Err("Invalid adaptive envelope observation".into());
        }
        let middle = position > bounds[0] + (bounds[1] - bounds[0]) * c.learning_inset_fraction
            && position < bounds[1] - (bounds[1] - bounds[0]) * c.learning_inset_fraction;
        // Central trials do not identify changing gravity/leverage near the ends.
        let a = if middle {
            self.deceleration(c, direction, time)
        } else {
            self.deceleration(c, direction, time)
                .min(c.fallback_deceleration_rad_s2)
        };
        let distance = if direction == 1 {
            bounds[1] - position
        } else {
            position - bounds[0]
        };
        let margin = c.boundary_margin_rad + c.position_uncertainty_rad;
        // Solve v*t + v²/(2*a) <= available distance. Use measured position, not target position.
        let available = (distance - margin).max(0.);
        let geometric =
            ((a * c.reaction_time_s).powi(2) + 2. * a * available).sqrt() - a * c.reaction_time_s;
        let demonstrated = if middle {
            self.demonstrated_speed(c, direction, time)
        } else {
            c.bootstrap_speed_rad_s
        };
        let exploration = if exploring && middle {
            c.trial_speed_growth
        } else {
            1.
        };
        let limit = requested
            .min(demonstrated * exploration)
            .min(geometric.max(0.));
        let outward = if direction == 1 {
            velocity.max(0.)
        } else {
            (-velocity).max(0.)
        };
        let stopping = outward * c.reaction_time_s + outward * outward / (2. * a) + margin;
        Ok(Envelope {
            permitted_speed_rad_s: limit,
            stopping_distance_rad: stopping,
            deceleration_rad_s2: a,
            measured_speed_limit_rad_s: demonstrated,
            accepted_stops: self.evidence(c, direction, time).len(),
            brake_now: outward > c.bootstrap_speed_rad_s * 0.4 && stopping >= distance,
            margin_rad: margin,
        })
    }
    pub fn observe(
        &mut self,
        c: &Config,
        time: f64,
        position: f64,
        velocity: f64,
        braking: bool,
        effort_limit: f64,
        bounds: [f64; 2],
    ) -> Result<(), String> {
        c.validate()?;
        if [time, position, velocity, effort_limit, bounds[0], bounds[1]]
            .iter()
            .any(|v| !v.is_finite())
            || time < 0.
            || !(0.0..=1.0).contains(&effort_limit)
            || bounds[0] >= bounds[1]
        {
            return Err("Invalid learning observation".into());
        }
        let context = (bounds[0], bounds[1], effort_limit);
        if self.context.is_some_and(|old| old != context) {
            self.reset("Pose bounds or effort ceiling changed; learning restarted");
        }
        self.context = Some(context);
        let d = usize::from(velocity >= 0.);
        let interior = position > bounds[0] + (bounds[1] - bounds[0]) * c.learning_inset_fraction
            && position < bounds[1] - (bounds[1] - bounds[0]) * c.learning_inset_fraction;
        if let Some((t, v)) = self.previous {
            if time <= t {
                return Err("Learning timestamps must increase".into());
            }
            if time - t >= 0.15 {
                let acceleration = (velocity - v) / (time - t);
                if interior && !braking && acceleration * velocity > 0. {
                    self.directions[d].acceleration_rad_s2 =
                        0.8 * self.directions[d].acceleration_rad_s2 + 0.2 * acceleration.abs();
                }
                self.previous = Some((time, velocity));
            }
        } else {
            self.previous = Some((time, velocity));
        }
        if braking && !self.was_braking && velocity.abs() >= c.bootstrap_speed_rad_s * 0.4 {
            let a = self.deceleration(c, d, time);
            self.episode = Some(Episode {
                direction: d,
                start_rad: position,
                speed_rad_s: velocity
                    .abs()
                    .min(self.motion_speed_limit_rad_s.unwrap_or(f64::INFINITY)),
                peak_rad: 0.,
                interior,
                quiet_window: None,
                predicted_rad: velocity.abs() * c.reaction_time_s
                    + velocity * velocity / (2. * a)
                    + c.position_uncertainty_rad,
            });
        }
        self.was_braking = braking;
        if !braking {
            self.episode = None;
        }
        if let Some(e) = &mut self.episode {
            let sign = if e.direction == 1 { 1. } else { -1. };
            e.peak_rad = e.peak_rad.max((position - e.start_rad) * sign);
            e.interior &= interior;
            if e.peak_rad > e.predicted_rad {
                // A worse-than-predicted stop immediately lowers trust, even near an edge.
                let weak = c.braking_safety_factor * e.speed_rad_s.powi(2)
                    / (2. * (e.peak_rad + c.position_uncertainty_rad));
                let evidence = &mut self.directions[e.direction];
                evidence.weakened_deceleration_rad_s2 = Some(
                    evidence
                        .weakened_deceleration_rad_s2
                        .unwrap_or(f64::INFINITY)
                        .min(weak),
                );
            }
            // Quantized encoders can report alternating velocities while holding a pose.
            // Require an entire one-second position window, not a single small velocity sample.
            let window = e.quiet_window.get_or_insert((time, position, position));
            window.1 = window.1.min(position);
            window.2 = window.2.max(position);
            if window.2 - window.1 > c.position_uncertainty_rad {
                *window = (time, position, position);
            }
            if time - window.0 >= 1.0 {
                let e = self.episode.take().unwrap();
                if e.interior {
                    let stops = &mut self.directions[e.direction].stops;
                    stops.push(StopEvidence {
                        speed_rad_s: e.speed_rad_s,
                        excursion_rad: e.peak_rad,
                        effective_deceleration_rad_s2: e.speed_rad_s.powi(2)
                            / (2. * (e.peak_rad + c.position_uncertainty_rad)),
                        completed_s: time,
                    });
                    if stops.len() > 64 {
                        stops.remove(0);
                    }
                    self.completed_stops += 1;
                }
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct Request {
    config: Config,
    model: Model,
    time_s: f64,
    angle_rad: f64,
    velocity_rad_s: f64,
    bounds_rad: [f64; 2],
    direction: usize,
    requested_speed_rad_s: f64,
    exploring: bool,
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), String> {
    registry.register_primitive(Descriptor::new("control.adaptive_braking_envelope","Direction-specific, evidence-limited angular speed envelope",vec![F::structured("config","rad,rad/s,rad/s²,s; fractions and stop counts dimensionless","Config"),F::structured("model","rad,rad/s,rad/s²,s","Model"),F::quantity("time_s",Q::Time,"scalar"),F::quantity("angle_rad",Q::Angle,"scalar"),F::quantity("velocity_rad_s",Q::AngularVelocity,"scalar"),F::quantity("bounds_rad",Q::Angle,"pair"),F::structured("direction","1","0 decreasing, 1 increasing"),F::quantity("requested_speed_rad_s",Q::AngularVelocity,"scalar"),F::structured("exploring","1","boolean")],vec![F::structured("$","rad,rad/s,rad/s²; count and boolean","Envelope")],&["Empirical stopping evidence is not a safety certification","Model must be reset on changed actuator/load/configuration; hard limits remain independent"]),|r:Request|r.model.envelope(&r.config,r.time_s,r.angle_rad,r.velocity_rad_s,r.bounds_rad,r.direction,r.requested_speed_rad_s,r.exploring))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        Config {
            bootstrap_speed_rad_s: 0.1,
            fallback_deceleration_rad_s2: 0.2,
            reaction_time_s: 0.2,
            position_uncertainty_rad: 0.001,
            boundary_margin_rad: 0.03,
            learning_inset_fraction: 0.25,
            minimum_stops: 3,
            braking_safety_factor: 0.5,
            trial_speed_growth: 1.25,
            maximum_evidence_age_s: 120.,
        }
    }
    fn stop(
        model: &mut Model,
        c: &Config,
        time: &mut f64,
        d: usize,
        speed: f64,
        accel: f64,
        origin: f64,
    ) {
        let sign = if d == 1 { 1. } else { -1. };
        let mut position = origin;
        *time += 0.02;
        model
            .observe(c, *time, position, sign * speed, false, 0.2, [0., 10.])
            .unwrap();
        let mut v = speed;
        for _ in 0..400 {
            *time += 0.02;
            model
                .observe(c, *time, position, sign * v, true, 0.2, [0., 10.])
                .unwrap();
            v = (v - accel * 0.02).max(0.);
            position += sign * v * 0.02;
        }
    }
    #[test]
    fn directional_evidence_never_extrapolates_speed_and_expires() {
        let c = config();
        let mut m = Model::default();
        let mut time = 0.;
        for _ in 0..3 {
            stop(&mut m, &c, &mut time, 1, 0.4, 0.8, 5.);
        }
        assert_eq!(m.completed_stops, 3);
        assert!(m.demonstrated_speed(&c, 1, time) > 0.39);
        assert_eq!(m.demonstrated_speed(&c, 0, time), 0.1);
        let center = m
            .envelope(&c, time, 5., 0.4, [0., 10.], 1, 10., false)
            .unwrap();
        assert!(center.permitted_speed_rad_s <= 0.4);
        let edge = m
            .envelope(&c, time, 9.95, 0.4, [0., 10.], 1, 10., true)
            .unwrap();
        assert!(edge.brake_now);
        assert!(edge.permitted_speed_rad_s < center.permitted_speed_rad_s);
        assert_eq!(m.demonstrated_speed(&c, 1, time + 121.), 0.1);
    }
    #[test]
    fn edge_stops_never_increase_confidence_and_weaker_braking_reduces_it() {
        let c = config();
        let mut m = Model::default();
        let mut time = 0.;
        for _ in 0..3 {
            stop(&mut m, &c, &mut time, 1, 0.4, 0.8, 5.);
        }
        let before = m.deceleration(&c, 1, time);
        stop(&mut m, &c, &mut time, 1, 0.4, 0.08, 8.);
        assert_eq!(m.completed_stops, 3);
        assert!(m.deceleration(&c, 1, time) < before);
        *&mut time += 0.02;
        m.observe(&c, time, 5., 0., false, 0.1, [0., 10.]).unwrap();
        assert_eq!(m.completed_stops, 0);
        assert_eq!(m.demonstrated_speed(&c, 1, time), 0.1);
    }
    #[test]
    fn invalid_numbers_and_parameters_are_rejected() {
        let mut c = config();
        let m = Model::default();
        assert!(
            m.envelope(&c, 0., 5., f64::NAN, [0., 10.], 1, 1., false)
                .is_err()
        );
        c.braking_safety_factor = 1.;
        assert!(c.validate().is_err());
    }
    #[test]
    fn noisy_velocity_cannot_promote_speed_above_the_trial_request() {
        let c = config();
        let mut m = Model::default();
        let mut time = 0.;
        for _ in 0..3 {
            m.constrain_observed_speed(0.12).unwrap();
            stop(&mut m, &c, &mut time, 1, 0.4, 0.8, 5.);
        }
        assert!(m.demonstrated_speed(&c, 1, time) <= 0.12);
        assert!(
            m.envelope(&c, time, 8., 0.1, [0., 10.], 1, 2., false)
                .unwrap()
                .permitted_speed_rad_s
                <= c.bootstrap_speed_rad_s
        );
    }
}

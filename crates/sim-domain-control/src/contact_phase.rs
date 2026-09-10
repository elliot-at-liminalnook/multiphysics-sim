//! Periodic body/foot reference with independently timed swing/stance phases.
//! This is a geometric search parameterization, not a contact or balance model.
use crate::trajectory::{Interpolation, Trajectory, TrajectoryConfig, TrajectorySample};
use crate::smooth_return::SmoothReturn;
use serde::{Deserialize, Serialize};
mod sequence;
pub use sequence::FootStep;
use sequence::PreparedFootSequence;
#[cfg(test)]
mod sequence_tests;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FootPhase {
    /// World foothold center, metres. Successive contacts advance by the cycle displacement.
    pub center_world_m: [f64; 3],
    /// Fraction of a cycle; each foot is independent of every other foot.
    pub phase_offset: f64,
    pub stance_fraction: f64,
    /// Mid-swing excursion added to the path, metres in world axes.
    pub swing_offset_world_m: [f64; 3],
    /// Optional C2 constant-speed-middle progress; absent retains exact quintic
    /// displacement. This changes policy geometry, not contact or actuator laws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ramp_fraction: Option<f64>,
    /// Further stance/swing pairs in the same cycle. Empty retains the legacy
    /// single-step path exactly. Each step's swing leads to the next touchdown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_steps: Vec<FootStep>,
}
impl FootPhase {
    /// Stable identity order: original step first, then configured additional steps.
    /// Sampling sorts by onset without changing these identities.
    pub fn steps(&self) -> impl Iterator<Item = FootStep> + '_ {
        std::iter::once(FootStep {
            center_world_m: self.center_world_m,
            phase_offset: self.phase_offset,
            stance_fraction: self.stance_fraction,
            swing_offset_world_m: self.swing_offset_world_m,
            return_ramp_fraction: self.return_ramp_fraction,
        }).chain(self.additional_steps.iter().cloned())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactPhaseConfig {
    pub period_s: f64,
    pub displacement_world_m: [f64; 3],
    /// Periodic xyz translation (m), then world rotation vector (rad).
    /// Rotation-vector derivatives are NOT angular velocity/acceleration.
    pub body: TrajectoryConfig,
    pub feet: Vec<FootPhase>,
}
impl ContactPhaseConfig {
    /// Express the same motion over a longer cycle to initialize independent
    /// multi-step optimization. No actuator/contact property or speed changes.
    pub fn repeated_cycle(&self, count: usize) -> Result<Self, String> {
        ContactPhaseMotion::new(self.clone())?;
        if count == 0 || count > 64 {
            return Err("cycle expansion requires 1..64 repetitions (allocation limit, not a physical bound)".into());
        }
        if count == 1 { return Ok(self.clone()); }
        let mut result = self.clone();
        let factor = count as f64;
        result.period_s *= factor;
        for value in &mut result.displacement_world_m { *value *= factor; }
        let controls = &self.body.keyframes[..self.body.keyframes.len() - 1];
        result.body.keyframes = (0..count).flat_map(|repeat| controls.iter().map(move |k| {
            let mut node = k.clone(); node.time_s += repeat as f64 * self.period_s; node
        })).collect();
        let mut last = result.body.keyframes[0].clone(); last.time_s = result.period_s;
        result.body.keyframes.push(last);
        for (input, output) in self.feet.iter().zip(&mut result.feet) {
            let steps = (0..count).flat_map(|repeat| input.steps().map(move |mut step| {
                step.phase_offset = (step.phase_offset + repeat as f64) / factor;
                step.stance_fraction /= factor; step
            })).collect::<Vec<_>>();
            output.phase_offset = steps[0].phase_offset;
            output.stance_fraction = steps[0].stance_fraction;
            output.additional_steps = steps[1..].to_vec();
        }
        ContactPhaseMotion::new(result.clone())?;
        Ok(result)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct FootSample {
    pub position_world_m: [f64; 3],
    pub velocity_world_m_s: [f64; 3],
    pub acceleration_world_m_s2: [f64; 3],
    pub in_contact: bool,
    pub phase: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContactPhaseSample {
    /// Periodic body output plus the commanded displacement*time/period.
    pub body: TrajectorySample,
    pub feet: Vec<FootSample>,
}
/// An interval with an unchanged set of stance feet. Phases are cycle fractions;
/// the last end may exceed one to represent the wrap across the cycle boundary.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ContactInterval {
    pub start_phase: f64,
    pub end_phase: f64,
}
pub struct ContactPhaseMotion {
    config: ContactPhaseConfig,
    body: Trajectory,
    returns: Vec<Option<SmoothReturn>>,
    sequences: Vec<Option<PreparedFootSequence>>,
}
impl ContactPhaseMotion {
    pub fn new(config: ContactPhaseConfig) -> Result<Self, String> {
        if !config.period_s.is_finite()
            || config.period_s <= 0.0
            || config.displacement_world_m.iter().any(|v| !v.is_finite())
            || config.feet.is_empty()
            || config.feet.iter().any(|f| {
                f.center_world_m
                    .iter()
                    .chain(&f.swing_offset_world_m)
                    .any(|v| !v.is_finite())
                    || !f.phase_offset.is_finite()
                    || !(0.0..1.0).contains(&f.phase_offset)
                    || !f.stance_fraction.is_finite()
                    || f.stance_fraction <= 0.0
                    || f.stance_fraction >= 1.0
            })
            || !matches!(
                config.body.interpolation,
                Interpolation::PeriodicCubicBSpline
            )
            || config
                .body
                .keyframes
                .last()
                .is_none_or(|k| k.time_s != config.period_s)
        {
            return Err(
                "finite periodic body and independent foot phases with 0<stance<1 required".into(),
            );
        }
        let body = Trajectory::new(config.body.clone())?;
        if body.dimension() != 6 {
            return Err("body requires xyz metres and rotation-vector radians".into());
        }
        let returns = config.feet.iter().map(|foot|
            foot.return_ramp_fraction.map(SmoothReturn::new).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        let sequences = config.feet.iter().map(|foot| {
            if foot.additional_steps.is_empty() { Ok(None) }
            else { PreparedFootSequence::new(foot.steps().collect()).map(Some) }
        }).collect::<Result<Vec<_>, String>>()?;
        Ok(Self { config, body, returns, sequences })
    }
    /// Keep coincident events as zero-duration intervals so an optimizer has a
    /// fixed number of slots. They carry no integration weight. Every positive
    /// interval must still pass feasibility checks, however short it becomes.
    pub fn contact_intervals(&self) -> Vec<ContactInterval> {
        let mut events = self
            .config
            .feet
            .iter()
            .flat_map(|f| f.steps())
            .flat_map(|f| {
                [
                    f.phase_offset,
                    (f.phase_offset + f.stance_fraction).rem_euclid(1.0),
                ]
            })
            .collect::<Vec<_>>();
        events.sort_by(f64::total_cmp);
        (0..events.len())
            .map(|i| ContactInterval {
                start_phase: events[i],
                end_phase: if i + 1 == events.len() {
                    events[0] + 1.0
                } else {
                    events[i + 1]
                },
            })
            .collect()
    }
    pub fn sample(&self, time_s: f64) -> Result<ContactPhaseSample, String> {
        if !time_s.is_finite() {
            return Err("finite sample time required".into());
        }
        let p = self.config.period_s;
        let cycles = time_s / p;
        if !cycles.is_finite() {
            return Err("nonfinite normalized phase".into());
        }
        let mut body = self.body.sample(time_s.rem_euclid(p))?;
        for i in 0..3 {
            body.values[i] += self.config.displacement_world_m[i] * cycles;
            body.rates[i] += self.config.displacement_world_m[i] / p;
        }
        let feet = self
            .config
            .feet
            .iter()
            .zip(&self.returns)
            .zip(&self.sequences)
            .map(|((f, smooth_return), sequence)| {
                if let Some(sequence) = sequence {
                    return sequence.sample(cycles, p, self.config.displacement_world_m);
                }
                let u = (cycles - f.phase_offset).rem_euclid(1.0);
                let contact = u < f.stance_fraction;
                let (s, ds, dds, b, db, ddb) = if contact {
                    (0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
                } else {
                    let v = (u - f.stance_fraction) / (1.0 - f.stance_fraction);
                    let rate = 1.0 / ((1.0 - f.stance_fraction) * p);
                    let (s, ds, dds) = if let Some(curve) = smooth_return {
                        let sample = curve.sample(v)?;
                        (sample[0], sample[1] * rate, sample[2] * rate * rate)
                    } else {
                        (v.powi(3) * (10.0 + v * (-15.0 + 6.0 * v)),
                         30.0 * v * v * (1.0 - v).powi(2) * rate,
                         60.0 * v * (1.0 - v) * (1.0 - 2.0 * v) * rate * rate)
                    };
                    (s, ds, dds,
                        64.0 * v.powi(3) * (1.0 - v).powi(3),
                        192.0 * v * v * (1.0 - v).powi(2) * (1.0 - 2.0 * v) * rate,
                        384.0 * v * (1.0 - v) * (1.0 - 5.0 * v + 5.0 * v * v) * rate * rate,
                    )
                };
                Ok(FootSample {
                    position_world_m: std::array::from_fn(|i| {
                        f.center_world_m[i]
                            + self.config.displacement_world_m[i]
                                * (cycles + s - u + f.stance_fraction / 2.0)
                            + f.swing_offset_world_m[i] * b
                    }),
                    velocity_world_m_s: std::array::from_fn(|i| {
                        self.config.displacement_world_m[i] * ds + f.swing_offset_world_m[i] * db
                    }),
                    acceleration_world_m_s2: std::array::from_fn(|i| {
                        self.config.displacement_world_m[i] * dds + f.swing_offset_world_m[i] * ddb
                    }),
                    in_contact: contact,
                    phase: u,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if body
            .values
            .iter()
            .chain(&body.rates)
            .chain(&body.accelerations)
            .chain(feet.iter().flat_map(|f| {
                f.position_world_m
                    .iter()
                    .chain(&f.velocity_world_m_s)
                    .chain(&f.acceleration_world_m_s2)
            }))
            .any(|v| !v.is_finite())
        {
            return Err("nonfinite reference output".into());
        }
        Ok(ContactPhaseSample { body, feet })
    }
    /// Explicit stance and swing interior samples for every configured step.
    pub fn phase_midpoints(&self) -> Vec<f64> {
        self.config.feet.iter().zip(&self.sequences).flat_map(|(foot, sequence)| {
            if let Some(sequence) = sequence { sequence.midpoints() }
            else { vec![
                (foot.phase_offset + 0.5 * foot.stance_fraction).rem_euclid(1.0),
                (foot.phase_offset + foot.stance_fraction + 0.5 * (1.0 - foot.stance_fraction)).rem_euclid(1.0),
            ] }
        }).collect()
    }
    /// Apply the chain rule for a controller's reference clock. A negative rate
    /// reverses the same geometric motion; zero rate/acceleration holds its pose.
    /// Clock acceleration has units 1/s because the reference clock uses seconds.
    pub fn sample_retimed(
        &self,
        reference_time_s: f64,
        phase_rate: f64,
        phase_acceleration_per_s: f64,
    ) -> Result<ContactPhaseSample, String> {
        if !phase_rate.is_finite() || !phase_acceleration_per_s.is_finite() {
            return Err("finite reference-clock rate and acceleration required".into());
        }
        let mut sample = self.sample(reference_time_s)?;
        for i in 0..sample.body.rates.len() {
            sample.body.accelerations[i] = sample.body.accelerations[i] * phase_rate * phase_rate
                + sample.body.rates[i] * phase_acceleration_per_s;
            sample.body.rates[i] *= phase_rate;
        }
        for f in &mut sample.feet {
            for i in 0..3 {
                f.acceleration_world_m_s2[i] =
                    f.acceleration_world_m_s2[i] * phase_rate * phase_rate
                        + f.velocity_world_m_s[i] * phase_acceleration_per_s;
                f.velocity_world_m_s[i] *= phase_rate;
            }
        }
        if sample
            .body
            .rates
            .iter()
            .chain(&sample.body.accelerations)
            .chain(sample.feet.iter().flat_map(|f| {
                f.velocity_world_m_s
                    .iter()
                    .chain(&f.acceleration_world_m_s2)
            }))
            .any(|v| !v.is_finite())
        {
            return Err("nonfinite retimed reference derivatives".into());
        }
        Ok(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::Keyframe;
    fn motion() -> ContactPhaseMotion {
        ContactPhaseMotion::new(ContactPhaseConfig {
            period_s: 0.8,
            displacement_world_m: [0.08, 0.02, 0.0],
            body: TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes: (0..=4)
                    .map(|i| Keyframe {
                        time_s: i as f64 * 0.2,
                        values: vec![0.0; 6],
                    })
                    .collect(),
            },
            feet: vec![FootPhase {
                center_world_m: [0.1, 0.2, 0.0],
                phase_offset: 0.17,
                stance_fraction: 0.6,
                swing_offset_world_m: [0.0, 0.03, 0.02],
                return_ramp_fraction: None,
                additional_steps: vec![],
            }],
        })
        .unwrap()
    }
    #[test]
    fn periodic_progress_and_stationary_stance() {
        let m = motion();
        for t in [-0.11, 0.21, 0.59, 1.2] {
            let a = m.sample(t).unwrap();
            let b = m.sample(t + 0.8).unwrap();
            for (i, d) in [0.08, 0.02, 0.0].iter().enumerate() {
                assert!(
                    (b.feet[0].position_world_m[i] - a.feet[0].position_world_m[i] - d).abs()
                        < 1e-14
                );
            }
        }
        let a = m.sample(0.2).unwrap();
        let b = m.sample(0.3).unwrap();
        assert!(a.feet[0].in_contact && b.feet[0].in_contact);
        for i in 0..3 {
            assert!((a.feet[0].position_world_m[i] - b.feet[0].position_world_m[i]).abs() < 1e-14);
            assert_eq!(a.feet[0].velocity_world_m_s[i], 0.0);
        }
    }
    #[test]
    fn derivatives_and_c2_phase_boundaries() {
        let m = motion();
        let h = 1e-6;
        for t in [0.68, 0.75, 0.80, 0.17 * 0.8, (0.17 + 0.6) * 0.8] {
            let a = m.sample(t - h).unwrap();
            let b = m.sample(t).unwrap();
            let c = m.sample(t + h).unwrap();
            for i in 0..3 {
                assert!(
                    ((c.feet[0].position_world_m[i] - a.feet[0].position_world_m[i]) / (2.0 * h)
                        - b.feet[0].velocity_world_m_s[i])
                        .abs()
                        < 1e-7
                );
                assert!(
                    ((c.feet[0].velocity_world_m_s[i] - a.feet[0].velocity_world_m_s[i])
                        / (2.0 * h)
                        - b.feet[0].acceleration_world_m_s2[i])
                        .abs()
                        < 1e-3
                );
            }
        }
        assert!(m.sample(f64::INFINITY).is_err());
    }
    #[test]
    fn smooth_return_keeps_stance_and_c2_derivatives() {
        let original = motion();
        let mut config = original.config.clone();
        let json = serde_json::to_value(&config).unwrap();
        assert!(json["feet"][0].get("return_ramp_fraction").is_none());
        config.feet[0].return_ramp_fraction = Some(0.25);
        let shaped = ContactPhaseMotion::new(config.clone()).unwrap();
        let start = (0.17 + 0.6) * 0.8;
        let duration = (1. - 0.6) * 0.8;
        for t in [0.2, 0.3, 0.5] {
            let a = original.sample(t).unwrap(); let b = shaped.sample(t).unwrap();
            assert_eq!(a.feet[0].position_world_m, b.feet[0].position_world_m);
            assert_eq!(b.feet[0].velocity_world_m_s, [0.; 3]);
            assert_eq!(a.body.values, b.body.values);
        }
        for u in [0., 0.1, 0.25, 0.5, 0.75, 0.9, 1.] {
            let t = start + duration * u; let h = 1e-6;
            let a = shaped.sample(t - h).unwrap(); let b = shaped.sample(t).unwrap();
            let c = shaped.sample(t + h).unwrap();
            for i in 0..3 {
                assert!(((c.feet[0].position_world_m[i] - a.feet[0].position_world_m[i]) / (2. * h)
                    - b.feet[0].velocity_world_m_s[i]).abs() < 1e-7);
                assert!(((c.feet[0].velocity_world_m_s[i] - a.feet[0].velocity_world_m_s[i]) / (2. * h)
                    - b.feet[0].acceleration_world_m_s2[i]).abs() < 1e-3);
                let q = original.sample(t).unwrap();
                assert_eq!(q.feet[0].position_world_m[2], b.feet[0].position_world_m[2]);
            }
        }
        config.feet[0].return_ramp_fraction = Some(0.6);
        assert!(ContactPhaseMotion::new(config).is_err());
    }
    #[test]
    fn intervals_expose_short_flight_and_preserve_coincident_events() {
        let mut config = motion().config;
        config.feet = (0..4)
            .map(|i| FootPhase {
                phase_offset: if i % 2 == 0 { 0.0 } else { 0.5 },
                stance_fraction: 0.5 - 1e-7,
                ..config.feet[0].clone()
            })
            .collect();
        let m = ContactPhaseMotion::new(config).unwrap();
        let intervals = m.contact_intervals();
        assert_eq!(intervals.len(), 8);
        assert_eq!(
            intervals
                .iter()
                .filter(|i| i.end_phase == i.start_phase)
                .count(),
            4
        );
        let mut flight_duration = 0.0;
        let mut total = 0.0;
        for i in intervals {
            let duration = i.end_phase - i.start_phase;
            total += duration;
            if duration == 0.0 {
                continue;
            }
            let contacts = |fraction: f64| {
                m.sample((i.start_phase + fraction * duration) * m.config.period_s)
                    .unwrap()
                    .feet
                    .iter()
                    .map(|f| f.in_contact)
                    .collect::<Vec<_>>()
            };
            let mid = contacts(0.5);
            assert_eq!(contacts(0.001), mid);
            assert_eq!(contacts(0.999), mid);
            if !mid.iter().any(|c| *c) {
                flight_duration += duration;
            }
        }
        assert!((total - 1.0).abs() < 1e-15);
        assert!((flight_duration - 2e-7).abs() < 1e-15);
        // This uniform grid misses both brief unsupported intervals.
        assert!((0..256).all(|i| {
            m.sample((i as f64 + 0.5) / 256.0 * m.config.period_s)
                .unwrap()
                .feet
                .iter()
                .any(|f| f.in_contact)
        }));
    }
    #[test]
    fn retimed_derivatives_include_reversal_and_clock_acceleration() {
        let m = motion();
        let (t, rate, acceleration, h) = (0.75, -0.8, 0.3, 1e-5);
        let s = m.sample_retimed(t, rate, acceleration).unwrap();
        let left = m.sample(t - rate * h + 0.5 * acceleration * h * h).unwrap();
        let right = m.sample(t + rate * h + 0.5 * acceleration * h * h).unwrap();
        for i in 0..3 {
            assert!(
                ((right.feet[0].position_world_m[i] - left.feet[0].position_world_m[i])
                    / (2.0 * h)
                    - s.feet[0].velocity_world_m_s[i])
                    .abs()
                    < 1e-7
            );
            assert!(
                ((right.feet[0].position_world_m[i] - 2.0 * s.feet[0].position_world_m[i]
                    + left.feet[0].position_world_m[i])
                    / (h * h)
                    - s.feet[0].acceleration_world_m_s2[i])
                    .abs()
                    < 2e-6
            );
            assert!(
                ((right.body.values[i] - left.body.values[i]) / (2.0 * h) - s.body.rates[i]).abs()
                    < 1e-7
            );
            assert!(
                ((right.body.values[i] - 2.0 * s.body.values[i] + left.body.values[i]) / (h * h)
                    - s.body.accelerations[i])
                    .abs()
                    < 2e-6
            );
        }
        let hold = m.sample_retimed(t, 0.0, 0.0).unwrap();
        assert!(
            hold.body
                .rates
                .iter()
                .chain(&hold.body.accelerations)
                .all(|v| *v == 0.0)
        );
        assert!(
            hold.feet
                .iter()
                .all(|f| f.velocity_world_m_s == [0.0; 3] && f.acceleration_world_m_s2 == [0.0; 3])
        );
        assert!(m.sample_retimed(t, f64::INFINITY, 0.0).is_err());
    }
}

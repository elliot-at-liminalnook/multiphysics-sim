//! One foot's contact schedule: one or more stance/swing pairs per cycle.
//! Where each foothold lies is the caller's (periodic drift or a steered
//! body path); timing, swing shape and lift are shared here.
use super::FootSample;
use crate::{planar::rotate, smooth_return::UnitProgress};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FootStep {
    /// Foothold center relative to the body path at the stance midpoint, metres.
    /// Periodic motion: touchdown is center + displacement * (cycle + onset + duration/2).
    pub center_world_m: [f64; 3],
    /// Touchdown phase in [0,1), relative to the common body cycle.
    pub phase_offset: f64,
    /// Stance duration as a fraction of the common cycle, not of this step.
    pub stance_fraction: f64,
    /// C2 mid-swing excursion from the path to the next touchdown, world metres
    /// (turned with the body heading when steered).
    pub swing_offset_world_m: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ramp_fraction: Option<f64>,
}

/// A foothold chosen by the caller: world position and the body heading it
/// was placed at (rad; zero for unsteered periodic motion).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub position_world_m: [f64; 3],
    pub heading_rad: f64,
}

/// Lift excursion shape over unit swing phase: 64 v³(1−v)³ (peak 1 at mid
/// swing), with first and second derivatives. C2 at liftoff and touchdown.
pub fn lift(v: f64) -> [f64; 3] {
    let r = 1. - v;
    [
        64. * v * v * v * r * r * r,
        192. * v * v * r * r * (1. - 2. * v),
        384. * v * r * (1. - 5. * v + 5. * v * v),
    ]
}

pub(crate) struct PreparedFootSequence {
    steps: Vec<FootStep>,
    progress: Vec<UnitProgress>,
}

impl PreparedFootSequence {
    pub fn new(mut steps: Vec<FootStep>) -> Result<Self, String> {
        if steps.is_empty()
            || steps.iter().any(|s| {
                !s.phase_offset.is_finite()
                    || !(0.0..1.0).contains(&s.phase_offset)
                    || !s.stance_fraction.is_finite()
                    || s.stance_fraction <= 0.0
                    || s.stance_fraction >= 1.0
                    || s.center_world_m
                        .iter()
                        .chain(&s.swing_offset_world_m)
                        .any(|x| !x.is_finite())
            })
        {
            return Err(
                "finite foot steps with normalized onsets and positive stance durations required"
                    .into(),
            );
        }
        steps.sort_by(|a, b| a.phase_offset.total_cmp(&b.phase_offset));
        for i in 0..steps.len() {
            if steps[i].phase_offset + steps[i].stance_fraction >= Self::next_onset(&steps, i) {
                return Err("each foot stance must end strictly before its next touchdown; overlapping or zero-duration swings are invalid".into());
            }
        }
        let progress = steps
            .iter()
            .map(|s| UnitProgress::new(s.return_ramp_fraction))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { steps, progress })
    }

    /// Steps sorted by touchdown phase.
    pub fn steps(&self) -> &[FootStep] {
        &self.steps
    }

    fn next_onset(steps: &[FootStep], i: usize) -> f64 {
        steps[(i + 1) % steps.len()].phase_offset + if i + 1 == steps.len() { 1.0 } else { 0.0 }
    }

    /// Mid-stance of step `i` in cycle `turn`, in cycles.
    pub fn midstance(&self, i: usize, turn: f64) -> f64 {
        turn + self.steps[i].phase_offset + self.steps[i].stance_fraction / 2.0
    }

    /// Longest time, in cycles, from any instant to the mid-stance of a
    /// foothold the foot is on or swinging to. A steered path must be fixed
    /// this far ahead for every placement to be final when it is first used.
    pub fn commitment(&self) -> f64 {
        (0..self.steps.len())
            .map(|i| {
                let next = (i + 1) % self.steps.len();
                let liftoff = self.steps[i].phase_offset + self.steps[i].stance_fraction;
                (Self::next_onset(&self.steps, i) + self.steps[next].stance_fraction / 2.0 - liftoff)
                    .max(self.steps[i].stance_fraction / 2.0)
            })
            .fold(0.0, f64::max)
    }

    pub fn midpoints(&self) -> Vec<f64> {
        let mut phases = Vec::with_capacity(2 * self.steps.len());
        for (i, s) in self.steps.iter().enumerate() {
            let liftoff = s.phase_offset + s.stance_fraction;
            phases.push((s.phase_offset + 0.5 * s.stance_fraction).rem_euclid(1.0));
            phases.push((0.5 * (liftoff + Self::next_onset(&self.steps, i))).rem_euclid(1.0));
        }
        phases
    }

    /// Sample at `cycles` (time / period). `place(step, turn)` gives the
    /// foothold of sorted step `step` in cycle `turn`.
    pub fn sample(
        &self,
        cycles: f64,
        period: f64,
        place: impl Fn(usize, f64) -> Placement,
    ) -> Result<FootSample, String> {
        let phase = cycles.rem_euclid(1.0);
        let after = self.steps.partition_point(|s| s.phase_offset <= phase);
        let (i, turn) = if after == 0 {
            (self.steps.len() - 1, cycles.floor() - 1.0)
        } else {
            (after - 1, cycles.floor())
        };
        let step = &self.steps[i];
        let local = cycles - (turn + step.phase_offset);
        let from = place(i, turn);
        if local < step.stance_fraction {
            return Ok(FootSample {
                position_world_m: from.position_world_m,
                velocity_world_m_s: [0.; 3],
                acceleration_world_m_s2: [0.; 3],
                in_contact: true,
                phase: local,
            });
        }
        let next = (i + 1) % self.steps.len();
        let next_turn = turn + if next == 0 { 1.0 } else { 0.0 };
        let duration = Self::next_onset(&self.steps, i) - step.phase_offset - step.stance_fraction;
        // Validated, nonoverlapping steps and the phase partition above place
        // this sample inside the swing. Subtracting cycle offsets can round a
        // touchdown's interpolation coordinate just above one. Bound this
        // derived coordinate before evaluating the endpoint derivatives.
        let v = ((local - step.stance_fraction) / duration).clamp(0.0, 1.0);
        let rate = 1.0 / (duration * period);
        let [s, ds, dds] = self.progress[i].sample(v)?;
        let [b, db, ddb] = lift(v);
        let to = place(next, next_turn);
        let xy = rotate(
            0.5 * (from.heading_rad + to.heading_rad),
            [step.swing_offset_world_m[0], step.swing_offset_world_m[1]],
        );
        let offset = [xy[0], xy[1], step.swing_offset_world_m[2]];
        let delta: [f64; 3] = std::array::from_fn(|a| to.position_world_m[a] - from.position_world_m[a]);
        Ok(FootSample {
            position_world_m: std::array::from_fn(|a| {
                from.position_world_m[a] + delta[a] * s + offset[a] * b
            }),
            velocity_world_m_s: std::array::from_fn(|a| (delta[a] * ds + offset[a] * db) * rate),
            acceleration_world_m_s2: std::array::from_fn(|a| {
                (delta[a] * dds + offset[a] * ddb) * rate * rate
            }),
            in_contact: false,
            phase: local,
        })
    }
}

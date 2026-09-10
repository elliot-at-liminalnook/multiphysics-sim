//! Multiple independently placed contacts per foot and periodic cycle.
use super::{FootSample, SmoothReturn};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FootStep {
    /// Foothold center relative to cycle drift at the stance midpoint, metres.
    /// Actual touchdown is center + displacement * (cycle + onset + duration/2).
    pub center_world_m: [f64; 3],
    /// Touchdown phase in [0,1), relative to the common body cycle.
    pub phase_offset: f64,
    /// Stance duration as a fraction of the common cycle, not of this step.
    pub stance_fraction: f64,
    /// C2 mid-swing excursion from the path to the next touchdown, world metres.
    pub swing_offset_world_m: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ramp_fraction: Option<f64>,
}

pub(super) struct PreparedFootSequence {
    steps: Vec<FootStep>,
    returns: Vec<Option<SmoothReturn>>,
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
            let next = steps[(i + 1) % steps.len()].phase_offset
                + if i + 1 == steps.len() { 1.0 } else { 0.0 };
            if next - steps[i].phase_offset <= steps[i].stance_fraction {
                return Err("each foot stance must end strictly before its next touchdown; overlapping or zero-duration swings are invalid".into());
            }
        }
        let returns = steps
            .iter()
            .map(|s| s.return_ramp_fraction.map(SmoothReturn::new).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { steps, returns })
    }

    pub fn midpoints(&self) -> Vec<f64> {
        let mut phases = Vec::with_capacity(2 * self.steps.len());
        for (i, s) in self.steps.iter().enumerate() {
            let next = self.steps[(i + 1) % self.steps.len()].phase_offset
                + if i + 1 == self.steps.len() { 1.0 } else { 0.0 };
            let liftoff = s.phase_offset + s.stance_fraction;
            phases.push((s.phase_offset + 0.5 * s.stance_fraction).rem_euclid(1.0));
            phases.push((0.5 * (liftoff + next)).rem_euclid(1.0));
        }
        phases
    }

    pub fn sample(
        &self,
        cycles: f64,
        period: f64,
        displacement: [f64; 3],
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
        let position = std::array::from_fn(|axis| {
            step.center_world_m[axis]
                + displacement[axis] * (turn + step.phase_offset + step.stance_fraction / 2.0)
        });
        if local < step.stance_fraction {
            return Ok(FootSample {
                position_world_m: position,
                velocity_world_m_s: [0.; 3],
                acceleration_world_m_s2: [0.; 3],
                in_contact: true,
                phase: local,
            });
        }
        let next = &self.steps[(i + 1) % self.steps.len()];
        let next_turn = turn + if i + 1 == self.steps.len() { 1.0 } else { 0.0 };
        let duration =
            next_turn - turn + next.phase_offset - step.phase_offset - step.stance_fraction;
        // Validated, nonoverlapping steps and the phase partition above place
        // this sample inside the swing. Subtracting cycle offsets can round a
        // touchdown's interpolation coordinate just above one. Bound this
        // derived coordinate before evaluating the endpoint derivatives.
        let v = ((local - step.stance_fraction) / duration).clamp(0.0, 1.0);
        let rate = 1.0 / (duration * period);
        let [s, ds, dds] = if let Some(curve) = &self.returns[i] {
            curve.sample(v)?
        } else {
            [
                v.powi(3) * (10.0 + v * (-15.0 + 6.0 * v)),
                30.0 * v * v * (1.0 - v).powi(2),
                60.0 * v * (1.0 - v) * (1.0 - 2.0 * v),
            ]
        };
        let bump = 64.0 * v.powi(3) * (1.0 - v).powi(3);
        let db = 192.0 * v * v * (1.0 - v).powi(2) * (1.0 - 2.0 * v);
        let ddb = 384.0 * v * (1.0 - v) * (1.0 - 5.0 * v + 5.0 * v * v);
        let delta: [f64; 3] = std::array::from_fn(|axis| {
            next.center_world_m[axis]
                + displacement[axis] * (next_turn + next.phase_offset + next.stance_fraction / 2.0)
                - position[axis]
        });
        Ok(FootSample {
            position_world_m: std::array::from_fn(|a| {
                position[a] + delta[a] * s + step.swing_offset_world_m[a] * bump
            }),
            velocity_world_m_s: std::array::from_fn(|a| {
                (delta[a] * ds + step.swing_offset_world_m[a] * db) * rate
            }),
            acceleration_world_m_s2: std::array::from_fn(|a| {
                (delta[a] * dds + step.swing_offset_world_m[a] * ddb) * rate * rate
            }),
            in_contact: false,
            phase: local,
        })
    }
}

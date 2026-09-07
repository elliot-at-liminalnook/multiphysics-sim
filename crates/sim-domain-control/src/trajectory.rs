//! Validated, deterministic reference trajectories. Values retain the units and
//! channel order declared by the caller; derivatives use seconds as time units.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    #[default]
    Linear,
    /// Quintic position interpolation with zero velocity/acceleration at knots.
    QuinticRestToRest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub time_s: f64,
    pub values: Vec<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrajectoryConfig {
    #[serde(default)]
    pub interpolation: Interpolation,
    pub keyframes: Vec<Keyframe>,
}
#[derive(Clone, Debug)]
pub struct Trajectory {
    config: TrajectoryConfig,
    dimension: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct TrajectorySample {
    pub values: Vec<f64>,
    pub rates: Vec<f64>,
    pub accelerations: Vec<f64>,
}
impl Trajectory {
    pub fn new(config: TrajectoryConfig) -> Result<Self, String> {
        let dimension = config.keyframes.first().map_or(0, |k| k.values.len());
        if dimension == 0
            || config.keyframes.iter().any(|k| {
                !k.time_s.is_finite()
                    || k.time_s < 0.0
                    || k.values.len() != dimension
                    || k.values.iter().any(|v| !v.is_finite())
            })
        {
            return Err(
                "trajectory requires finite, nonempty, consistently sized keyframes".into(),
            );
        }
        for w in config.keyframes.windows(2) {
            let h = w[1].time_s - w[0].time_s;
            if h <= 0.0
                || !h.is_finite()
                || w[0].values.iter().zip(&w[1].values).any(|(a, b)| {
                    let d = b - a;
                    !d.is_finite() || !(d / h * 1.875).is_finite() || !(d / h / h * 6.0).is_finite()
                })
            {
                return Err(
                    "trajectory times must increase and derivatives must remain finite".into(),
                );
            }
        }
        Ok(Self { config, dimension })
    }
    /// Both supported interpolation laws remain between each pair of endpoint
    /// values. These bounds therefore cover reference values throughout time,
    /// not the achieved motion, dependent coordinates or collision clearance.
    pub fn validate_value_bounds(
        &self,
        bounds: &[(Option<f64>, Option<f64>)],
    ) -> Result<(), String> {
        if bounds.len() != self.dimension
            || bounds.iter().any(|(lo, hi)| {
                lo.is_some_and(|x| !x.is_finite())
                    || hi.is_some_and(|x| !x.is_finite())
                    || lo.zip(*hi).is_some_and(|(lo, hi)| lo > hi)
            })
        {
            return Err("invalid trajectory value bounds".into());
        }
        for k in &self.config.keyframes {
            for (i, (value, (lo, hi))) in k.values.iter().zip(bounds).enumerate() {
                if lo.is_some_and(|lo| *value < lo) || hi.is_some_and(|hi| *value > hi) {
                    return Err(format!(
                        "trajectory coordinate {i} at {}s = {value} exceeds bounds {lo:?}..{hi:?}",
                        k.time_s
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn dimension(&self) -> usize {
        self.dimension
    }
    /// Before/after the time domain, hold the endpoint with zero derivatives.
    /// Linear-knot derivatives are right-sided; the final knot is held.
    pub fn sample(&self, time_s: f64) -> Result<TrajectorySample, String> {
        if !time_s.is_finite() || time_s < 0.0 {
            return Err("invalid trajectory sample time".into());
        }
        let ks = &self.config.keyframes;
        let held = if time_s < ks[0].time_s {
            Some(&ks[0])
        } else if time_s >= ks.last().unwrap().time_s {
            ks.last()
        } else {
            None
        };
        if let Some(k) = held {
            return Ok(TrajectorySample {
                values: k.values.clone(),
                rates: vec![0.0; self.dimension],
                accelerations: vec![0.0; self.dimension],
            });
        }
        let end = ks.partition_point(|k| k.time_s <= time_s);
        let (a, b) = (&ks[end - 1], &ks[end]);
        let h = b.time_s - a.time_s;
        let s = (time_s - a.time_s) / h;
        let (position, rate, acceleration) = match self.config.interpolation {
            Interpolation::Linear => (s, 1.0, 0.0),
            Interpolation::QuinticRestToRest => (
                s * s * s * (10.0 + s * (-15.0 + 6.0 * s)),
                30.0 * s * s * (1.0 - s) * (1.0 - s),
                60.0 * s * (1.0 - s) * (1.0 - 2.0 * s),
            ),
        };
        Ok(TrajectorySample {
            values: a
                .values
                .iter()
                .zip(&b.values)
                .map(|(a, b)| a + (b - a) * position)
                .collect(),
            rates: a
                .values
                .iter()
                .zip(&b.values)
                .map(|(a, b)| (b - a) / h * rate)
                .collect(),
            accelerations: a
                .values
                .iter()
                .zip(&b.values)
                .map(|(a, b)| (b - a) / h / h * acceleration)
                .collect(),
        })
    }
}

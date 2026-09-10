//! Shared C2 periodic reference with an explicit displacement per cycle.
//! Channels retain caller-declared units; this does not prescribe contact modes.
use crate::trajectory::{Interpolation, Trajectory, TrajectoryConfig, TrajectorySample};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodicDriftConfig {
    pub periodic: TrajectoryConfig,
    pub displacement_per_cycle: Vec<f64>,
}
#[derive(Clone, Debug)]
pub struct PeriodicDriftTrajectory {
    periodic: Trajectory,
    displacement: Vec<f64>,
    period_s: f64,
}
impl PeriodicDriftTrajectory {
    pub fn new(config: PeriodicDriftConfig) -> Result<Self, String> {
        if !matches!(
            config.periodic.interpolation,
            Interpolation::PeriodicCubicBSpline
        ) {
            return Err("periodic drift requires the shared periodic cubic B-spline".into());
        }
        let periodic = Trajectory::new(config.periodic.clone())?;
        let period_s = config.periodic.keyframes.last().unwrap().time_s;
        if config.displacement_per_cycle.len() != periodic.dimension()
            || config
                .displacement_per_cycle
                .iter()
                .any(|v| !v.is_finite() || !(v / period_s).is_finite())
        {
            return Err("finite dimension-matched cycle displacement required".into());
        }
        Ok(Self {
            periodic,
            displacement: config.displacement_per_cycle,
            period_s,
        })
    }
    pub fn sample(&self, time_s: f64) -> Result<TrajectorySample, String> {
        let mut sample = self.periodic.sample(time_s)?;
        for j in 0..self.displacement.len() {
            sample.values[j] += self.displacement[j] * (time_s / self.period_s);
            sample.rates[j] += self.displacement[j] / self.period_s;
        }
        if sample
            .values
            .iter()
            .chain(&sample.rates)
            .any(|v| !v.is_finite())
        {
            return Err("nonfinite translating periodic reference".into());
        }
        Ok(sample)
    }
    pub fn period_s(&self) -> f64 {
        self.period_s
    }
    /// Preserve the complete translating curve while doubling its controls.
    pub fn refined_config(&self) -> Result<PeriodicDriftConfig, String> {
        Ok(PeriodicDriftConfig {
            periodic: self.periodic.refined_periodic_config()?,
            displacement_per_cycle: self.displacement.clone(),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::Keyframe;
    #[test]
    fn translating_reference_closes_derivatives_and_matches_finite_differences() {
        let config = PeriodicDriftConfig {
            periodic: TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes: [0., 1., -0.5, 0.25, 0.]
                    .into_iter()
                    .enumerate()
                    .map(|(k, v)| Keyframe {
                        time_s: k as f64 * 0.25,
                        values: vec![v, 2. * v],
                    })
                    .collect(),
            },
            displacement_per_cycle: vec![0.3, -0.1],
        };
        let curve = PeriodicDriftTrajectory::new(config.clone()).unwrap();
        let a = curve.sample(0.).unwrap();
        let b = curve.sample(1.).unwrap();
        assert_eq!(a.rates, b.rates);
        assert_eq!(a.accelerations, b.accelerations);
        for j in 0..2 {
            assert!((b.values[j] - a.values[j] - config.displacement_per_cycle[j]).abs() < 1e-14);
        }
        for t in [0.13, 0.25, 0.499, 1., 1.37] {
            let h = 1e-5;
            let m = curve.sample(t - h).unwrap();
            let p = curve.sample(t + h).unwrap();
            let c = curve.sample(t).unwrap();
            for j in 0..2 {
                assert!(((p.values[j] - m.values[j]) / (2. * h) - c.rates[j]).abs() < 1e-7);
                assert!(((p.rates[j] - m.rates[j]) / (2. * h) - c.accelerations[j]).abs() < 0.003);
            }
        }
        let mut invalid = config;
        invalid.displacement_per_cycle.pop();
        assert!(PeriodicDriftTrajectory::new(invalid).is_err());
        assert!(curve.sample(f64::INFINITY).is_err());
    }
    #[test]
    fn periodic_knot_insertion_preserves_position_velocity_and_acceleration() {
        let config = PeriodicDriftConfig {
            periodic: TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes: (0..=7)
                    .map(|k| Keyframe {
                        time_s: k as f64 * 0.073,
                        values: vec![((k % 7) as f64 * 1.3).sin(), ((k % 7) as f64 * 0.7).cos()],
                    })
                    .collect(),
            },
            displacement_per_cycle: vec![0.07, -0.03],
        };
        let original = PeriodicDriftTrajectory::new(config).unwrap();
        let mut curve = original.clone();
        for _ in 0..3 {
            curve = PeriodicDriftTrajectory::new(curve.refined_config().unwrap()).unwrap();
            for k in 0..=2000 {
                let t = k as f64 * original.period_s() / 1000.;
                let a = original.sample(t).unwrap();
                let b = curve.sample(t).unwrap();
                for j in 0..2 {
                    assert!((a.values[j] - b.values[j]).abs() < 1e-13);
                    assert!((a.rates[j] - b.rates[j]).abs() < 1e-11);
                    assert!((a.accelerations[j] - b.accelerations[j]).abs() < 1e-8);
                }
            }
        }
    }
}

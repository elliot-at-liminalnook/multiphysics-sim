//! Empirical first-order pulse-response identification, independent of a motor
//! topology. This is a reduced input/output fit, not an inferred physical motor.
//! Output is the integral of a lagged rate. Caller declares units, timing,
//! weights, bounds and experimental validity; no extrapolation is certified.
use crate::least_squares::{
    LeastSquaresConfig, LeastSquaresResult, VariableBound, bounded_least_squares,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PulseResponse {
    /// Steady output rate per input unit above the deadband.
    pub gain: f64,
    pub deadband: f64,
    pub drive_tau_s: f64,
    pub release_tau_s: f64,
    /// Combined delay relative to the caller's command/observation clock.
    pub delay_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PulseObservation {
    pub input: f64,
    pub duration_s: f64,
    /// [time since command, output displacement, positive residual scale].
    pub samples: Vec<[f64; 3]>,
}
impl PulseResponse {
    pub fn from_values(v: &[f64]) -> Result<Self, String> {
        if v.len() != 5
            || v.iter().any(|x| !x.is_finite())
            || v[0] < 0.
            || v[1] < 0.
            || v[2] <= 0.
            || v[3] <= 0.
            || v[4] < 0.
        {
            return Err(
                "finite nonnegative gain/deadband/delay and positive lag times required".into(),
            );
        }
        Ok(Self {
            gain: v[0],
            deadband: v[1],
            drive_tau_s: v[2],
            release_tau_s: v[3],
            delay_s: v[4],
        })
    }
    /// Exact integrated first-order step and release; no numerical time step.
    pub fn predict(&self, input: f64, duration_s: f64, time_s: f64) -> Result<[f64; 2], String> {
        Self::from_values(&[
            self.gain,
            self.deadband,
            self.drive_tau_s,
            self.release_tau_s,
            self.delay_s,
        ])?;
        if !input.is_finite() || !duration_s.is_finite() || duration_s <= 0. || !time_s.is_finite()
        {
            return Err("invalid pulse/observation clock".into());
        }
        let t = (time_s - self.delay_s).max(0.);
        let steady = self.gain * input.signum() * (input.abs() - self.deadband).max(0.);
        let on = t.min(duration_s);
        let fraction = -(-on / self.drive_tau_s).exp_m1();
        let mut rate = steady * fraction;
        let mut position = steady * (on - self.drive_tau_s * fraction);
        if t > duration_s {
            let fraction = -(-(t - duration_s) / self.release_tau_s).exp_m1();
            position += rate * self.release_tau_s * fraction;
            rate *= 1. - fraction;
        }
        if !position.is_finite() || !rate.is_finite() {
            return Err("nonfinite response".into());
        }
        Ok([position, rate])
    }
}
pub fn fit_pulses(
    observations: &[PulseObservation],
    bounds: &[VariableBound],
    starts: &[Vec<f64>],
    config: &LeastSquaresConfig,
) -> Result<LeastSquaresResult, String> {
    if observations.len() < 2 || starts.is_empty() || bounds.len() != 5 {
        return Err("multiple pulses, five bounds and explicit starts required".into());
    }
    for p in observations {
        if !p.input.is_finite()
            || !p.duration_s.is_finite()
            || p.duration_s <= 0.
            || p.samples.len() < 3
            || p.samples
                .iter()
                .any(|r| r.iter().any(|v| !v.is_finite()) || r[2] <= 0.)
            || p.samples.windows(2).any(|w| w[0][0] >= w[1][0])
        {
            return Err("invalid pulse observations or residual scales".into());
        }
    }
    if observations
        .iter()
        .flat_map(|p| &p.samples)
        .all(|r| r[1].abs() < 1e-12)
    {
        return Err("no detected response; dynamics are not identifiable".into());
    }
    let mut best: Option<LeastSquaresResult> = None;
    for start in starts {
        let candidate = bounded_least_squares(start, bounds, config, |v| {
            let model = PulseResponse::from_values(v)?;
            observations
                .iter()
                .flat_map(|p| p.samples.iter().map(move |sample| (p, sample)))
                .map(|(p, r)| {
                    Ok((model.predict(p.input, p.duration_s, r[0])?[0] - r[1])
                        / r[2]
                        / (p.samples.len() as f64).sqrt())
                })
                .collect()
        })?;
        if best.as_ref().is_none_or(|b| candidate.cost < b.cost) {
            best = Some(candidate);
        }
    }
    Ok(best.unwrap())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pulse_integral_continuity_symmetry_and_asymptote() {
        let m = PulseResponse::from_values(&[4., 0.02, 0.04, 0.02, 0.01]).unwrap();
        assert_eq!(m.predict(0.1, 0.15, 0.005).unwrap(), [0., 0.]);
        let a = m.predict(0.1, 0.15, 0.16).unwrap();
        let b = m.predict(0.1, 0.15, 0.16 + 1e-10).unwrap();
        assert!((a[0] - b[0]).abs() < 1e-9);
        assert!((a[1] - b[1]).abs() < 1e-8);
        let p = m.predict(0.1, 0.15, 1.).unwrap();
        let n = m.predict(-0.1, 0.15, 1.).unwrap();
        assert_eq!(p[0], -n[0]);
        let expected = 4. * (0.1 - 0.02) * (0.15 + (0.02 - 0.04) * (1. - (-0.15_f64 / 0.04).exp()));
        assert!((p[0] - expected).abs() < 1e-12);
        assert!(p[1].abs() < 1e-12);
        assert_eq!(m.predict(0.01, 0.15, 1.).unwrap(), [0., 0.]);
    }
    #[test]
    fn recovers_known_dynamics_from_irregular_samples_and_rejects_flat_data() {
        let truth = PulseResponse::from_values(&[5., 0.02, 0.04, 0.018, 0.012]).unwrap();
        let observations: Vec<_> = [0.025, 0.1, 0.2]
            .into_iter()
            .map(|input| PulseObservation {
                input,
                duration_s: 0.15,
                samples: (0..65)
                    .map(|i| {
                        let t = i as f64 * 0.004 + (i % 3) as f64 * 0.0007;
                        [t, truth.predict(input, 0.15, t).unwrap()[0], 0.001]
                    })
                    .collect(),
            })
            .collect();
        let bounds: Vec<_> = [
            (0., 10.),
            (0., 0.08),
            (0.001, 0.2),
            (0.001, 0.2),
            (0., 0.04),
        ]
        .into_iter()
        .map(|(lower, upper)| VariableBound { lower, upper })
        .collect();
        let config = LeastSquaresConfig {
            maximum_iterations: 100,
            maximum_evaluations: 1200,
            difference_step: 1e-5,
            initial_damping: 0.001,
            gradient_tolerance: 1e-7,
        };
        let r = fit_pulses(
            &observations,
            &bounds,
            &[vec![4., 0.01, 0.025, 0.025, 0.005]],
            &config,
        )
        .unwrap();
        for (a, b) in r.values.iter().zip([5., 0.02, 0.04, 0.018, 0.012]) {
            assert!((a - b).abs() < 1e-5, "{:?}", r.values);
        }
        let mut flat = observations.clone();
        for p in &mut flat {
            for r in &mut p.samples {
                r[1] = 0.;
            }
        }
        assert!(
            fit_pulses(
                &flat,
                &bounds,
                &[vec![4., 0.01, 0.025, 0.025, 0.005]],
                &config
            )
            .is_err()
        );
        flat[0].samples[1][0] = -1.;
        assert!(
            fit_pulses(
                &flat,
                &bounds,
                &[vec![4., 0.01, 0.025, 0.025, 0.005]],
                &config
            )
            .is_err()
        );
    }
}

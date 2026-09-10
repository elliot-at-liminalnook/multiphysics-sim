//! Fit post-hoc Gaussian standard-deviation scales from excluded predictions.
//! The caller owns data separation. Adaptive observations from changing models
//! are useful diagnostics, but do not establish calibrated future coverage.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PredictionObservation {
    pub mean: Vec<f64>,
    pub std: Vec<f64>,
    pub actual: Vec<f64>,
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct StandardDeviationFit {
    pub samples: usize,
    pub std_scales: Vec<f64>,
    /// Scaled minus original mean Gaussian NLL; common normalization terms cancel.
    pub mean_nll_changes: Vec<f64>,
}

/// With fixed means, minimize log(s) + mean(((actual-mean)/std)^2)/(2*s^2).
/// The positive finite optimum is RMS standardized error, separately per output.
/// Actual, mean and std must share physical units in each column; scales are unitless.
/// Zero original deviations or all-zero errors have no supported positive fit.
pub fn fit_standard_deviation_scales(
    observations: &[PredictionObservation],
) -> Result<StandardDeviationFit, String> {
    let dimensions = observations.first().map_or(0, |o| o.mean.len());
    if dimensions == 0 {
        return Err("nonempty prediction observations required".into());
    }
    let mut mean_square = vec![0.; dimensions];
    for observation in observations {
        if observation.mean.len() != dimensions
            || observation.std.len() != dimensions
            || observation.actual.len() != dimensions
            || observation.evidence.trim().is_empty()
        {
            return Err("matching prediction dimensions and evidence required".into());
        }
        for (j, square) in mean_square.iter_mut().enumerate() {
            let (mean, std, actual) = (
                observation.mean[j],
                observation.std[j],
                observation.actual[j],
            );
            if !mean.is_finite() || !actual.is_finite() || !std.is_finite() || std <= 0. {
                return Err("finite predictions and positive standard deviations required".into());
            }
            let error = (actual - mean) / std;
            *square += error.powi(2) / observations.len() as f64;
        }
    }
    if mean_square.iter().any(|s| !s.is_finite() || *s <= 0.) {
        return Err("positive finite standardized squared error required for every output".into());
    }
    let std_scales: Vec<_> = mean_square.iter().map(|s| s.sqrt()).collect();
    let mean_nll_changes = std_scales
        .iter()
        .zip(&mean_square)
        .map(|(s, square)| s.ln() + 0.5 - 0.5 * square)
        .collect();
    Ok(StandardDeviationFit {
        samples: observations.len(),
        std_scales,
        mean_nll_changes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observations() -> Vec<PredictionObservation> {
        [-2., 2., -2., 2.]
            .into_iter()
            .map(|z| PredictionObservation {
                mean: vec![3., 5.],
                std: vec![0.25, 2.],
                actual: vec![3. + z * 0.25, 5. + z * 0.5],
                evidence: "analytic excluded prediction".into(),
            })
            .collect()
    }

    #[test]
    fn scales_minimize_gaussian_nll_and_preserve_units() {
        let data = observations();
        let fit = fit_standard_deviation_scales(&data).unwrap();
        assert_eq!(fit.std_scales, vec![2., 0.5]);
        for (j, &scale) in fit.std_scales.iter().enumerate() {
            let nll = |s: f64| {
                data.iter()
                    .map(|o| s.ln() + 0.5 * ((o.actual[j] - o.mean[j]) / (o.std[j] * s)).powi(2))
                    .sum::<f64>()
                    / data.len() as f64
            };
            assert!(nll(scale) < nll(scale * 0.9));
            assert!(nll(scale) < nll(scale * 1.1));
            assert!((fit.mean_nll_changes[j] - (nll(scale) - nll(1.))).abs() < 1e-14);
        }
        let mut converted = data;
        for row in &mut converted {
            row.mean[0] *= 1000.;
            row.std[0] *= 1000.;
            row.actual[0] *= 1000.;
        }
        assert_eq!(
            fit_standard_deviation_scales(&converted)
                .unwrap()
                .std_scales,
            fit.std_scales
        );
    }

    #[test]
    fn rejects_degenerate_or_incomplete_calibration_data() {
        assert!(fit_standard_deviation_scales(&[]).is_err());
        for std in [0., -1., f64::NAN, f64::INFINITY] {
            let mut data = observations();
            data[0].std[0] = std;
            assert!(fit_standard_deviation_scales(&data).is_err());
        }
        let mut data = observations();
        data[0].mean.pop();
        assert!(fit_standard_deviation_scales(&data).is_err());
        let mut data = observations();
        data[0].evidence.clear();
        assert!(fit_standard_deviation_scales(&data).is_err());
        let mut data = observations();
        for row in &mut data {
            row.actual = row.mean.clone();
        }
        assert!(fit_standard_deviation_scales(&data).is_err());
    }
}

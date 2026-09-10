//! Empirical scalar input/output calibration for experiment proposals.
//! Units belong to the caller. A fitted response is not a dynamics guarantee.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AffineResponse {
    pub mean_input: f64,
    pub mean_output: f64,
    pub gain: f64,
    pub residual_rms: f64,
    pub observed_input_range: [f64; 2],
    pub samples: usize,
}
impl AffineResponse {
    pub fn fit(samples: &[[f64; 2]]) -> Result<Self, String> {
        if samples.len() < 2 || samples.iter().flatten().any(|v| !v.is_finite()) {
            return Err("at least two finite input/output observations required".into());
        }
        let (mut x_mean, mut y_mean, mut variance, mut covariance) = (0., 0., 0., 0.);
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for (i, [x, y]) in samples.iter().enumerate() {
            let dx = x - x_mean;
            let dy = y - y_mean;
            x_mean += dx / (i + 1) as f64;
            y_mean += dy / (i + 1) as f64;
            variance += dx * (x - x_mean);
            covariance += dx * (y - y_mean);
            low = low.min(*x);
            high = high.max(*x);
        }
        if !variance.is_finite() || variance <= 0. {
            return Err("input variation required to identify a scalar response".into());
        }
        let gain = covariance / variance;
        let residual_rms = (samples
            .iter()
            .map(|[x, y]| (y - (y_mean + gain * (x - x_mean))).powi(2))
            .sum::<f64>()
            / samples.len() as f64)
            .sqrt();
        if !gain.is_finite()
            || !residual_rms.is_finite()
            || !x_mean.is_finite()
            || !y_mean.is_finite()
        {
            return Err("nonfinite affine response fit".into());
        }
        Ok(Self {
            mean_input: x_mean,
            mean_output: y_mean,
            gain,
            residual_rms,
            observed_input_range: [low, high],
            samples: samples.len(),
        })
    }
    pub fn predict(&self, input: f64) -> Result<f64, String> {
        let output = self.mean_output + self.gain * (input - self.mean_input);
        if !input.is_finite() || !output.is_finite() {
            return Err("nonfinite response prediction".into());
        }
        Ok(output)
    }
    /// Propose an input for a target output without clamping it to an invented
    /// bound. The caller must check its actual command domain and extrapolation.
    pub fn input_for(&self, target: f64) -> Result<f64, String> {
        let input = self.mean_input + (target - self.mean_output) / self.gain;
        if self.gain == 0. || !target.is_finite() || !input.is_finite() {
            return Err("finite target and nonzero identified response gain required".into());
        }
        Ok(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifies_response_and_respects_unit_changes() {
        let data: Vec<_> = [-0.02, 0., 0.02]
            .into_iter()
            .map(|x| [x, 0.04 + 1.8 * x])
            .collect();
        let model = AffineResponse::fit(&data).unwrap();
        assert!((model.gain - 1.8).abs() < 1e-14);
        assert!(model.residual_rms < 1e-14);
        let input = model.input_for(0.).unwrap();
        assert!((input + 0.04 / 1.8).abs() < 1e-14);
        assert!(model.predict(input).unwrap().abs() < 1e-14);
        let unit = 180. / std::f64::consts::PI;
        let degrees =
            AffineResponse::fit(&data.iter().map(|[x, y]| [x * unit, *y]).collect::<Vec<_>>())
                .unwrap();
        assert!((degrees.input_for(0.).unwrap() / unit - input).abs() < 1e-14);
    }
    #[test]
    fn residuals_expose_non_affine_response() {
        let fit = AffineResponse::fit(&[[-1., 2.], [0., 0.], [1., 2.]]).unwrap();
        assert!(fit.residual_rms > 0.9);
        assert!(fit.input_for(0.).is_err());
        assert!(AffineResponse::fit(&[[0., 1.], [0., 2.]]).is_err());
        assert!(AffineResponse::fit(&[[0., 1.], [f64::NAN, 2.]]).is_err());
        assert!(AffineResponse::fit(&[[0., 1.]]).is_err());
    }
}

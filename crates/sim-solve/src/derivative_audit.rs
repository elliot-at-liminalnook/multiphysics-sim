//! Central-difference convergence diagnostics. This reports evidence, not a
//! derivative accuracy certificate; nonsmooth models need one-sided analysis.
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct DifferenceSample {
    pub step: f64,
    pub derivative_l2: f64,
    pub derivative_change_relative: Option<f64>,
    pub largest_derivative_change_row: Option<usize>,
    pub symmetric_remainder_l2: f64,
    pub residual_cost_slope: f64,
    pub forward_cost_slope: f64,
    pub backward_cost_slope: f64,
}
/// `direction` is an explicit physical-coordinate displacement per unit step.
/// Steps must decrease; evaluation errors propagate without fabricated values.
pub fn central_difference_audit(
    initial: &[f64],
    direction: &[f64],
    steps: &[f64],
    mut evaluate: impl FnMut(&[f64]) -> Result<Vec<f64>, String>,
) -> Result<Vec<DifferenceSample>, String> {
    if initial.is_empty()
        || initial.len() != direction.len()
        || initial.iter().chain(direction).any(|x| !x.is_finite())
        || direction.iter().all(|x| *x == 0.)
        || steps.len() < 2
        || steps.iter().any(|h| !h.is_finite() || *h <= 0.)
        || steps.windows(2).any(|w| w[1] >= w[0])
    {
        return Err(
            "finite matched coordinates, nonzero direction and decreasing positive steps required"
                .into(),
        );
    }
    let base = evaluate(initial)?;
    let valid = |r: &[f64]| {
        !r.is_empty()
            && r.len() == base.len()
            && r.iter().all(|v| v.is_finite())
            && squared(r).is_finite()
    };
    if !valid(&base) {
        return Err("finite nonempty residuals required".into());
    }
    let cost = 0.5 * squared(&base);
    let mut previous: Option<Vec<f64>> = None;
    let mut samples = vec![];
    for &h in steps {
        let plus = evaluate(
            &initial
                .iter()
                .zip(direction)
                .map(|(x, d)| x + h * d)
                .collect::<Vec<_>>(),
        )?;
        let minus = evaluate(
            &initial
                .iter()
                .zip(direction)
                .map(|(x, d)| x - h * d)
                .collect::<Vec<_>>(),
        )?;
        if !valid(&plus) || !valid(&minus) {
            return Err("probe residuals changed dimension or became nonfinite".into());
        }
        let derivative = plus
            .iter()
            .zip(&minus)
            .map(|(p, m)| (p - m) / (2. * h))
            .collect::<Vec<_>>();
        let norm = squared(&derivative).sqrt();
        let (change, row) = if let Some(old) = &previous {
            let delta = derivative
                .iter()
                .zip(old)
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>();
            (
                Some(squared(&delta).sqrt() / norm.max(squared(old).sqrt()).max(1e-300)),
                delta
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                    .map(|(i, _)| i),
            )
        } else {
            (None, None)
        };
        let remainder = plus
            .iter()
            .zip(&minus)
            .zip(&base)
            .map(|((p, m), b)| (p + m - 2. * b) * 0.5)
            .collect::<Vec<_>>();
        let sample = DifferenceSample {
            step: h,
            derivative_l2: norm,
            derivative_change_relative: change,
            largest_derivative_change_row: row,
            symmetric_remainder_l2: squared(&remainder).sqrt(),
            residual_cost_slope: base.iter().zip(&derivative).map(|(r, d)| r * d).sum(),
            forward_cost_slope: (0.5 * squared(&plus) - cost) / h,
            backward_cost_slope: (cost - 0.5 * squared(&minus)) / h,
        };
        if [
            sample.derivative_l2,
            sample.symmetric_remainder_l2,
            sample.residual_cost_slope,
            sample.forward_cost_slope,
            sample.backward_cost_slope,
        ]
        .iter()
        .any(|x| !x.is_finite())
        {
            return Err("nonfinite derivative diagnostics".into());
        }
        samples.push(sample);
        previous = Some(derivative);
    }
    Ok(samples)
}
fn squared(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn analytic_polynomial_has_second_order_remainder_and_converging_slope() {
        let samples = central_difference_audit(&[0.4], &[2.], &[1e-2, 1e-3, 1e-4], |x| {
            Ok(vec![x[0].powi(3), x[0] * x[0]])
        })
        .unwrap();
        // d/dh [(.4+2h)^3, (.4+2h)^2] = [.96,1.6].
        let expected = 0.4_f64.powi(3) * 0.96 + 0.16 * 1.6;
        assert!((samples[2].residual_cost_slope - expected).abs() < 1e-8);
        assert!(
            (samples[0].symmetric_remainder_l2 / samples[1].symmetric_remainder_l2 - 100.).abs()
                < 1e-6
        );
        assert!(
            samples[2].derivative_change_relative.unwrap()
                < samples[1].derivative_change_relative.unwrap() / 90.
        );
    }
    #[test]
    fn discontinuity_is_exposed_and_invalid_probes_are_rejected() {
        let samples = central_difference_audit(&[0.], &[1.], &[1e-2, 1e-3, 1e-4], |x| {
            Ok(vec![if x[0] >= 0. { 1. } else { 0. }])
        })
        .unwrap();
        assert!(samples[2].derivative_change_relative.unwrap() > 0.89);
        assert_eq!(samples[2].symmetric_remainder_l2, 0.5);
        assert!(
            central_difference_audit(&[0.], &[1.], &[1e-2, 1e-3], |x| if x[0] < 0. {
                Err("domain".into())
            } else {
                Ok(vec![x[0]])
            })
            .is_err()
        );
    }
}

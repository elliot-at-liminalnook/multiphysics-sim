//! Interior residuals for least-squares solvers that reject invalid trials.
//! This protects only the inequalities supplied by the caller, not unsampled
//! constraints. It requires a strictly feasible initial point and can stall.

/// For dimensionless inequalities `g < 0`, return `sqrt(weight) / (-g)`.
/// The resulting least-squares cost is `weight / (2*g*g)` per row. Invalid
/// weights, boundary/exterior states and nonfinite residuals fail closed;
/// there is no clipped slack or extension outside the feasible domain.
pub fn reciprocal_inequality_barrier(g: &[f64], weight: f64) -> Result<Vec<f64>, String> {
    if !weight.is_finite() || weight <= 0. {
        return Err("barrier weight must be finite and positive".into());
    }
    g.iter()
        .map(|&g| {
            if !g.is_finite() || g >= 0. {
                return Err("barrier requires strictly negative finite inequalities".into());
            }
            let residual = weight.sqrt() / -g;
            if !residual.is_finite() {
                return Err("nonfinite reciprocal barrier residual".into());
            }
            Ok(residual)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::least_squares::{bounded_least_squares, LeastSquaresConfig, VariableBound};

    #[test]
    fn exact_residuals_and_strict_domain() {
        assert_eq!(reciprocal_inequality_barrier(&[-2., -0.5], 4.).unwrap(), vec![1., 4.]);
        for g in [0., -0., 1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -f64::from_bits(1)] {
            assert!(reciprocal_inequality_barrier(&[g], 4.).is_err());
        }
        for w in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(reciprocal_inequality_barrier(&[-1.], w).is_err());
        }
    }

    #[test]
    fn unsafe_trials_are_rejected_and_interior_stationary_point_is_reached() {
        let weight = 0.001;
        let mut outside = 0;
        let result = bounded_least_squares(
            &[0.],
            &[VariableBound { lower: -3., upper: 3. }],
            &LeastSquaresConfig {
                maximum_iterations: 100,
                maximum_evaluations: 2000,
                difference_step: 1e-6,
                initial_damping: 1e-4,
                gradient_tolerance: 1e-6,
            },
            |x| {
                if x[0] >= 1. { outside += 1; }
                let mut residuals = vec![x[0] - 2.];
                residuals.extend(reciprocal_inequality_barrier(&[x[0] - 1.], weight)?);
                Ok(residuals)
            },
        ).unwrap();
        let x = result.values[0];
        assert!(outside > 0 && result.rejected_evaluations >= outside);
        assert!(x > 0.8 && x < 1.);
        assert!(((x - 2.) + weight / (1. - x).powi(3)).abs() < 1e-5);
    }
}

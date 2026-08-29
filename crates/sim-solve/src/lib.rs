//! Small, deterministic nonlinear solver used by the first coupling island.

use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NewtonConfig {
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub max_iterations: usize,
    pub min_line_search: f64,
}

impl Default for NewtonConfig {
    fn default() -> Self {
        Self {
            absolute_tolerance: 1.0e-10,
            relative_tolerance: 1.0e-8,
            max_iterations: 18,
            min_line_search: 1.0 / 256.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SolveDiagnostics {
    pub iterations: usize,
    pub residual_norm: f64,
    pub line_search_reductions: usize,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum SolveError {
    #[error("residual dimension {actual} does not match unknown dimension {expected}")]
    Dimension { expected: usize, actual: usize },
    #[error("Newton Jacobian is singular at iteration {iteration}")]
    Singular { iteration: usize },
    #[error("Newton did not converge after {iterations} iterations; residual={residual:e}")]
    NotConverged { iterations: usize, residual: f64 },
    #[error("residual contains a non-finite value")]
    NonFinite,
}

pub fn solve_newton<F>(
    unknowns: &mut [f64],
    config: NewtonConfig,
    residual: F,
) -> Result<SolveDiagnostics, SolveError>
where
    F: Fn(&[f64], &mut [f64]),
{
    let n = unknowns.len();
    let mut r = vec![0.0; n];
    let mut perturbed_r = vec![0.0; n];
    let mut candidate_r = vec![0.0; n];
    let mut reductions = 0;

    residual(unknowns, &mut r);
    finite(&r)?;
    let initial_norm = infinity_norm(&r).max(1.0);

    for iteration in 0..=config.max_iterations {
        let norm = infinity_norm(&r);
        let scale = unknowns
            .iter()
            .fold(1.0_f64, |current, value| current.max(value.abs()));
        if norm <= config.absolute_tolerance + config.relative_tolerance * initial_norm * scale {
            return Ok(SolveDiagnostics {
                iterations: iteration,
                residual_norm: norm,
                line_search_reductions: reductions,
            });
        }
        if iteration == config.max_iterations {
            return Err(SolveError::NotConverged {
                iterations: iteration,
                residual: norm,
            });
        }

        let mut jacobian = DMatrix::zeros(n, n);
        for column in 0..n {
            let original = unknowns[column];
            let epsilon = f64::EPSILON.sqrt() * (1.0 + original.abs());
            unknowns[column] = original + epsilon;
            residual(unknowns, &mut perturbed_r);
            unknowns[column] = original;
            finite(&perturbed_r)?;
            for row in 0..n {
                jacobian[(row, column)] = (perturbed_r[row] - r[row]) / epsilon;
            }
        }

        let rhs = -DVector::from_column_slice(&r);
        let delta = jacobian
            .lu()
            .solve(&rhs)
            .ok_or(SolveError::Singular { iteration })?;

        let old = unknowns.to_vec();
        let old_norm = norm;
        let mut alpha = 1.0;
        loop {
            for index in 0..n {
                unknowns[index] = old[index] + alpha * delta[index];
            }
            residual(unknowns, &mut candidate_r);
            finite(&candidate_r)?;
            if infinity_norm(&candidate_r) < old_norm || alpha <= config.min_line_search {
                r.copy_from_slice(&candidate_r);
                break;
            }
            alpha *= 0.5;
            reductions += 1;
        }
    }

    unreachable!()
}

fn finite(values: &[f64]) -> Result<(), SolveError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(SolveError::NonFinite)
    }
}

fn infinity_norm(values: &[f64]) -> f64 {
    values
        .iter()
        .fold(0.0_f64, |current, value| current.max(value.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_nonlinear_system() {
        let mut x = [1.0, 1.0];
        let diagnostics = solve_newton(&mut x, NewtonConfig::default(), |x, r| {
            r[0] = x[0] * x[0] + x[1] - 5.0;
            r[1] = x[0] + x[1] * x[1] - 5.0;
        })
        .unwrap();
        assert!((x[0] - 1.791_287_847).abs() < 1.0e-7);
        assert!((x[1] - 1.791_287_847).abs() < 1.0e-7);
        assert!(diagnostics.residual_norm < 1.0e-7);
    }
}

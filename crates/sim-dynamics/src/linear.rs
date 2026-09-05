//! Linearisation of a [`System`] about a state, and what falls out of it:
//! the pencil `(E, A)` with `E = ∂r/∂ẋ`, `A = ∂r/∂x`, eigenvalues of the
//! differential part after eliminating algebraic unknowns, and transfer
//! functions between chosen input rows and output combinations.

use crate::System;
use nalgebra::{Complex, DMatrix, DVector};

/// `r(t, x + δx, ẋ + δẋ) ≈ r + A δx + E δẋ` at `(t, x, rate)`.
pub struct Linearisation {
    pub e: DMatrix<f64>,
    pub a: DMatrix<f64>,
    pub residual: DVector<f64>,
    /// Algebraic unknowns as the system declares them (else inferred from `E`).
    pub algebraic: Vec<bool>,
}

pub fn linearise<S: System>(system: &S, t: f64, x: &[f64], rate: &[f64]) -> Linearisation {
    let n = x.len();
    let mut e = DMatrix::zeros(n, n);
    let mut a = DMatrix::zeros(n, n);
    let mut base = vec![0.0; n];
    system.residual(t, x, rate, &mut base);
    let mut parts = crate::JacobianParts::default();
    if system.jacobian(t, x, rate, &mut parts) {
        let (da, de) = parts.dense(n);
        a = da;
        e = de;
    } else {
        let mut probe = vec![0.0; n];
        let mut xs = x.to_vec();
        let mut rs = rate.to_vec();
        for c in 0..n {
            let eps = 1.0e-6 * (1.0 + x[c].abs());
            xs[c] += eps;
            system.residual(t, &xs, rate, &mut probe);
            xs[c] = x[c];
            for r in 0..n {
                a[(r, c)] = (probe[r] - base[r]) / eps;
            }
            let eps = 1.0e-6 * (1.0 + rate[c].abs());
            rs[c] += eps;
            system.residual(t, x, &rs, &mut probe);
            rs[c] = rate[c];
            for r in 0..n {
                e[(r, c)] = (probe[r] - base[r]) / eps;
            }
        }
    }
    let scale = e.iter().fold(0.0_f64, |m, v| m.max(v.abs())).max(1.0e-300);
    let algebraic = system.algebraic().unwrap_or_else(|| (0..n).map(|c| (0..n).all(|r| e[(r, c)].abs() < 1.0e-9 * scale)).collect());
    Linearisation { e, a, residual: DVector::from_vec(base), algebraic }
}

impl Linearisation {
    /// Finite eigenvalues of the pencil `λE + A = 0` (the DAE's dynamics),
    /// by shift-invert: with `M = (A + σE)⁻¹E`, each eigenvalue μ of M gives
    /// λ = σ − 1/μ, and infinite eigenvalues (algebraic constraints) map to
    /// μ = 0 and are discarded. Works for any index-1 or index-2 structure,
    /// multipliers included.
    pub fn eigenvalues(&self) -> Vec<Complex<f64>> {
        let n = self.e.nrows();
        let e_scale = self.e.iter().fold(0.0_f64, |m, v| m.max(v.abs())).max(1.0e-300);
        let a_scale = self.a.iter().fold(0.0_f64, |m, v| m.max(v.abs())).max(1.0e-300);
        // Try a few shifts around the natural rate scale until the pencil is regular.
        for k in 0..6 {
            let sigma = (a_scale / e_scale) * [0.37, -0.71, 1.9, -3.3, 7.1, -0.13][k];
            let shifted = &self.a + sigma * &self.e;
            let Some(inverse) = shifted.try_inverse() else { continue };
            let m = inverse * &self.e;
            let mu = m.complex_eigenvalues();
            let floor = 1.0e-9 / e_scale.max(1.0e-300);
            let finite: Vec<Complex<f64>> = mu.iter().filter(|mu| mu.norm() > floor * 0.0 + 1.0e-12).map(|mu| Complex::new(sigma, 0.0) - Complex::new(1.0, 0.0) / mu).collect();
            if !finite.is_empty() || n == 0 {
                return finite;
            }
        }
        Vec::new()
    }

    /// Transfer function `y(s)/u(s)` where `u` enters the residual as
    /// `−b·u` and `y = cᵀ x`: `G(s) = cᵀ (sE + A)⁻¹ b`.
    pub fn transfer(&self, b: &DVector<f64>, c: &DVector<f64>, s: Complex<f64>) -> Complex<f64> {
        let n = self.e.nrows();
        let pencil = DMatrix::from_fn(n, n, |i, j| Complex::new(self.a[(i, j)], 0.0) + s * self.e[(i, j)]);
        let rhs = DVector::from_fn(n, |i, _| Complex::new(b[i], 0.0));
        match pencil.lu().solve(&rhs) {
            Some(x) => (0..n).map(|i| c[i] * x[i]).sum(),
            None => Complex::new(f64::NAN, f64::NAN),
        }
    }
}

/// Largest real part and its frequency among `eigenvalues`.
pub fn leading_mode(eigenvalues: &[Complex<f64>]) -> (f64, f64) {
    eigenvalues.iter().map(|e| (e.re, e.im.abs())).fold((f64::NEG_INFINITY, 0.0), |m, e| if e.0 > m.0 { e } else { m })
}

//! Finite-difference Jacobians that exploit a known sparsity pattern.
//!
//! Columns that touch disjoint rows are perturbed together (greedy graph
//! colouring), so an island of many small behaviors costs a handful of
//! residual evaluations per Jacobian instead of one per unknown.

use nalgebra::DMatrix;

/// Rows touched by each column (column `j` affects `rows[j]`).
#[derive(Debug, Clone)]
pub struct Sparsity {
    pub rows: Vec<Vec<usize>>,
    colours: Vec<usize>,
    colour_count: usize,
}

impl Sparsity {
    pub fn new(rows: Vec<Vec<usize>>) -> Self {
        let n = rows.len();
        // Greedy colouring: two columns may share a colour when no row is
        // touched by both.
        let mut colours = vec![usize::MAX; n];
        let mut colour_count = 0;
        let mut row_owner: Vec<Vec<usize>> = Vec::new();
        for column in 0..n {
            let mut colour = 0;
            'search: loop {
                if colour == colour_count {
                    colour_count += 1;
                    row_owner.push(Vec::new());
                }
                if rows[column].iter().all(|r| !row_owner[colour].contains(r)) {
                    colours[column] = colour;
                    for r in &rows[column] {
                        row_owner[colour].push(*r);
                    }
                    break 'search;
                }
                colour += 1;
            }
        }
        Self { rows, colours, colour_count }
    }

    pub fn colours(&self) -> usize {
        self.colour_count
    }

    /// Fill `jacobian` by forward differences, `colours()` residual calls.
    pub fn finite_difference(
        &self,
        x: &mut [f64],
        base: &[f64],
        jacobian: &mut DMatrix<f64>,
        residual: impl FnMut(&[f64], &mut [f64]),
    ) {
        self.finite_difference_with(x, base, jacobian, &|_, value: f64| 1.0e-6 * (1.0 + value.abs()), residual);
    }

    /// As [`Self::finite_difference`], with a caller-chosen perturbation
    /// `epsilon(column, value)`.
    pub fn finite_difference_with(
        &self,
        x: &mut [f64],
        base: &[f64],
        jacobian: &mut DMatrix<f64>,
        epsilon: &dyn Fn(usize, f64) -> f64,
        mut residual: impl FnMut(&[f64], &mut [f64]),
    ) {
        let n = x.len();
        let mut perturbed = vec![0.0; base.len()];
        let mut epsilons = vec![0.0; n];
        jacobian.fill(0.0);
        for colour in 0..self.colour_count {
            let originals = x.to_vec();
            for column in (0..n).filter(|c| self.colours[*c] == colour) {
                let eps = epsilon(column, x[column]);
                epsilons[column] = eps;
                x[column] += eps;
            }
            residual(x, &mut perturbed);
            x.copy_from_slice(&originals);
            for column in (0..n).filter(|c| self.colours[*c] == colour) {
                for &row in &self.rows[column] {
                    jacobian[(row, column)] = (perturbed[row] - base[row]) / epsilons[column];
                }
            }
        }
    }
}

impl Sparsity {
    /// As [`Self::finite_difference_with`], writing triplets into a sparse
    /// Jacobian: the pattern's rows for each column, nothing else.
    pub fn finite_difference_sparse(
        &self,
        x: &mut [f64],
        base: &[f64],
        jacobian: &mut sim_solve::SparseJacobian,
        epsilon: &dyn Fn(usize, f64) -> f64,
        mut residual: impl FnMut(&[f64], &mut [f64]),
    ) {
        let n = x.len();
        let mut perturbed = vec![0.0; base.len()];
        let mut epsilons = vec![0.0; n];
        jacobian.clear();
        jacobian.n = n;
        for colour in 0..self.colour_count {
            let originals = x.to_vec();
            for column in (0..n).filter(|c| self.colours[*c] == colour) {
                let eps = epsilon(column, x[column]);
                epsilons[column] = eps;
                x[column] += eps;
            }
            residual(x, &mut perturbed);
            x.copy_from_slice(&originals);
            for column in (0..n).filter(|c| self.colours[*c] == colour) {
                for &row in &self.rows[column] {
                    jacobian.add(row, column, (perturbed[row] - base[row]) / epsilons[column]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coloured_differences_match_dense_ones() {
        // Tridiagonal: three colours suffice.
        let n: usize = 6;
        let rows: Vec<Vec<usize>> = (0..n).map(|j| (j.saturating_sub(1)..(j + 2).min(n)).collect()).collect();
        let sparsity = Sparsity::new(rows);
        assert!(sparsity.colours() <= 3);
        let f = |x: &[f64], r: &mut [f64]| {
            for i in 0..n {
                r[i] = x[i] * x[i] + if i > 0 { x[i - 1] } else { 0.0 } - if i + 1 < n { x[i + 1] } else { 0.0 };
            }
        };
        let mut x = (0..n).map(|i| i as f64 * 0.3).collect::<Vec<_>>();
        let mut base = vec![0.0; n];
        f(&x, &mut base);
        let mut j = DMatrix::zeros(n, n);
        sparsity.finite_difference(&mut x, &base, &mut j, f);
        for i in 0..n {
            assert!((j[(i, i)] - 2.0 * x[i]).abs() < 1.0e-4);
            if i > 0 {
                assert!((j[(i, i - 1)] - 1.0).abs() < 1.0e-6);
            }
        }
    }
}

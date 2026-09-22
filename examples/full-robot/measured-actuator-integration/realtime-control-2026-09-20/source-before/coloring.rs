//! Numerical derivative grouping from caller-declared residual structure.
//! Each declaration must hold throughout its associated probe domain.
use crate::{
    NewtonAudit, NewtonConfig, SolveDiagnostics, SolveError, SparseJacobian,
    solve_newton_cached_audited,
};
use std::ops::Range;

/// Greedy groups with at most one participating column per residual row.
/// The caller must derive support from equations throughout each simultaneous
/// probe. Numerical zeros at one point do not establish independence. Groups
/// may be rebuilt for fixed-time probes without changing an NLP's global pattern.
pub fn group_disjoint_columns(
    column_rows: &[Vec<usize>],
    row_count: usize,
) -> Result<Vec<Vec<usize>>, &'static str> {
    let mut occupied: Vec<Vec<bool>> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (column, rows) in column_rows.iter().enumerate() {
        if rows.iter().any(|r| *r >= row_count)
            || rows
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != rows.len()
        {
            return Err("invalid or duplicate column-support row");
        }
        let group = occupied
            .iter()
            .position(|used| rows.iter().all(|r| !used[*r]));
        let group = group.unwrap_or_else(|| {
            occupied.push(vec![false; row_count]);
            groups.push(Vec::new());
            groups.len() - 1
        });
        for &row in rows {
            occupied[group][row] = true;
        }
        groups[group].push(column);
    }
    Ok(groups)
}

#[cfg(test)]
mod disjoint_tests {
    use super::*;
    #[test]
    fn nonlinear_grouped_probes_match_individual_columns() {
        let groups = group_disjoint_columns(&[vec![0], vec![0], vec![1], vec![1]], 2).unwrap();
        assert_eq!(groups, vec![vec![0, 2], vec![1, 3]]);
        let x = [0.2_f64, 0.3, 0.4, 0.5];
        let f = |v: [f64; 4]| [v[0].sin() + v[1].exp(), v[2] * v[2] + v[3].powi(3)];
        for group in groups {
            for sign in [-1., 1.] {
                let mut combined = x;
                for &col in &group {
                    combined[col] += sign * 1e-4 * (col + 1) as f64;
                }
                for &col in &group {
                    let mut single = x;
                    single[col] = combined[col];
                    assert_eq!(f(single)[col / 2], f(combined)[col / 2]);
                }
            }
        }
        assert!(group_disjoint_columns(&[vec![0, 0]], 2).is_err());
        assert!(group_disjoint_columns(&[vec![2]], 2).is_err());
        assert!(group_disjoint_columns(&[], 0).unwrap().is_empty());
    }
}

#[derive(Clone, Debug)]
pub struct BlockDiagonalColoring {
    blocks: Vec<Range<usize>>,
    dimension: usize,
    colors: usize,
}

impl BlockDiagonalColoring {
    /// Consecutive blocks partition both unknowns and residual rows. Every row
    /// in a block must depend only on columns in that block. This declaration
    /// must come from equation structure; observed numerical zeros do not prove it.
    pub fn new(sizes: &[usize]) -> Result<Self, &'static str> {
        let mut blocks = Vec::with_capacity(sizes.len());
        let mut dimension: usize = 0;
        for &size in sizes {
            if size == 0 {
                return Err("Jacobian blocks must be nonempty");
            }
            let end = dimension
                .checked_add(size)
                .ok_or("Jacobian dimension overflow")?;
            blocks.push(dimension..end);
            dimension = end;
        }
        Ok(Self {
            blocks,
            dimension,
            colors: sizes.iter().copied().max().unwrap_or(0),
        })
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }
    pub fn colors(&self) -> usize {
        self.colors
    }

    /// Same forward-difference steps as ordinary numerical Newton. One probe
    /// per color changes at most one column in each independent block. Always
    /// restores the caller's unknowns, including after nonfinite probe results.
    pub fn assemble<F>(
        &self,
        x: &mut [f64],
        base: &[f64],
        residual: F,
        out: &mut SparseJacobian,
    ) -> Result<(), SolveError>
    where
        F: Fn(&[f64], &mut [f64]),
    {
        if x.len() != self.dimension || base.len() != self.dimension {
            return Err(SolveError::Dimension {
                expected: self.dimension,
                actual: if x.len() != self.dimension {
                    x.len()
                } else {
                    base.len()
                },
            });
        }
        out.clear();
        out.n = self.dimension;
        let mut probe = vec![0.0; self.dimension];
        let mut changed = Vec::with_capacity(self.blocks.len());
        for color in 0..self.colors {
            changed.clear();
            for (index, block) in self.blocks.iter().enumerate() {
                if color >= block.len() {
                    continue;
                }
                let col = block.start + color;
                let original = x[col];
                let epsilon = 1e-6 * (1.0 + original.abs());
                changed.push((index, col, original, epsilon));
                x[col] = original + epsilon;
            }
            residual(x, &mut probe);
            for &(_, col, original, _) in &changed {
                x[col] = original;
            }
            if probe.iter().chain(base).any(|v| !v.is_finite()) {
                return Err(SolveError::NonFinite);
            }
            for &(index, col, _, epsilon) in &changed {
                for row in self.blocks[index].clone() {
                    out.add(row, col, (probe[row] - base[row]) / epsilon);
                }
            }
        }
        Ok(())
    }

    /// An independent one-column-at-a-time audit of the declared zeros at this
    /// state. Returns the largest off-block derivative. Use a perturbation
    /// sweep and representative modes; a passing audit is not structural proof.
    pub fn audit_off_block<F>(
        &self,
        x: &[f64],
        epsilon: f64,
        residual: F,
    ) -> Result<f64, SolveError>
    where
        F: Fn(&[f64], &mut [f64]),
    {
        if x.len() != self.dimension {
            return Err(SolveError::Dimension {
                expected: self.dimension,
                actual: x.len(),
            });
        }
        if !epsilon.is_finite() || epsilon <= 0.0 {
            return Err(SolveError::NonFinite);
        }
        let mut base = vec![0.0; x.len()];
        residual(x, &mut base);
        if base.iter().any(|v| !v.is_finite()) {
            return Err(SolveError::NonFinite);
        }
        let mut probe = base.clone();
        let mut trial = x.to_vec();
        let mut maximum = 0.0_f64;
        for block in &self.blocks {
            for col in block.clone() {
                let h = epsilon * (1.0 + x[col].abs());
                trial[col] = x[col] + h;
                residual(&trial, &mut probe);
                trial[col] = x[col];
                if probe.iter().any(|v| !v.is_finite()) {
                    return Err(SolveError::NonFinite);
                }
                for row in 0..x.len() {
                    if !block.contains(&row) {
                        maximum = maximum.max(((probe[row] - base[row]) / h).abs());
                    }
                }
            }
        }
        Ok(maximum)
    }
}

/// Newton with compressed numerical probes for declared independent blocks.
/// It retains ordinary Newton's scaling, line search and residual/correction
/// acceptance. Unknown structure must use the ordinary numerical entry point.
pub fn solve_newton_numeric_colored<F>(
    x: &mut [f64],
    config: NewtonConfig,
    residual: F,
    coloring: &BlockDiagonalColoring,
) -> Result<SolveDiagnostics, SolveError>
where
    F: Fn(&[f64], &mut [f64]),
{
    solve_newton_numeric_colored_scaled_audited(
        x,
        config,
        residual,
        coloring,
        &|_, value| 1.0 + value.abs(),
        None,
    )
}

/// Colored numerical Newton with a caller-declared correction scale and an
/// optional observational audit. The same raw residual bounds still apply.
pub fn solve_newton_numeric_colored_scaled_audited<F>(
    x: &mut [f64],
    config: NewtonConfig,
    residual: F,
    coloring: &BlockDiagonalColoring,
    step_scale: &dyn Fn(usize, f64) -> f64,
    audit: Option<&mut NewtonAudit>,
) -> Result<SolveDiagnostics, SolveError>
where
    F: Fn(&[f64], &mut [f64]),
{
    if x.len() != coloring.dimension {
        return Err(SolveError::Dimension {
            expected: coloring.dimension,
            actual: x.len(),
        });
    }
    solve_newton_cached_audited(
        x,
        config,
        &residual,
        |x, base, out| {
            if coloring.assemble(x, base, &residual, out).is_err() {
                out.add(0, 0, f64::NAN);
            }
        },
        step_scale,
        &mut None,
        audit,
    )
}

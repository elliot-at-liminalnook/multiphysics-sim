//! Small, deterministic nonlinear solver used by the first coupling island.

use nalgebra::DMatrix;
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
    let mut perturbed_r = vec![0.0; n];
    solve_newton_scaled(unknowns, config, &residual, |x, base, jacobian| {
        for column in 0..n {
            let original = x[column];
            // A model residual may itself contain finite-difference reference
            // terms (the leg's first CRBA-backed Coriolis implementation does).
            // A 1e-6 relative perturbation stays above that round-off floor
            // while remaining small compared with the timestep equations.
            let epsilon = 1.0e-6 * (1.0 + original.abs());
            x[column] = original + epsilon;
            residual(x, &mut perturbed_r);
            x[column] = original;
            for row in 0..n {
                jacobian[(row, column)] = (perturbed_r[row] - base[row]) / epsilon;
            }
        }
    }, &|_, value| 1.0 + value.abs())
}

/// Newton with a caller-supplied Jacobian assembler: `jacobian(x, r(x), J)`
/// fills `J` at `x` given the already-evaluated residual `r(x)`. The
/// assembler may perturb `x` as long as it restores it.
pub fn solve_newton_with_jacobian<F, J>(
    unknowns: &mut [f64],
    config: NewtonConfig,
    residual: F,
    jacobian_at: J,
) -> Result<SolveDiagnostics, SolveError>
where
    F: Fn(&[f64], &mut [f64]),
    J: FnMut(&mut [f64], &[f64], &mut DMatrix<f64>),
{
    solve_newton_scaled(unknowns, config, residual, jacobian_at, &|_, value| 1.0 + value.abs())
}

/// As [`solve_newton_with_jacobian`], with `step_scale(i, x_i)` giving the
/// size below which a change to unknown `i` is negligible (a differential
/// unknown in an implicit step is scaled by the step, since its change
/// also sets a rate).
/// A Jacobian as summed triplets `(row, column, value)`: what the solver
/// factorises. Sparse throughout, so an island of thousands of unknowns
/// costs what its couplings cost, not `n²` storage and `n³` factorisation.
#[derive(Debug, Clone, Default)]
pub struct SparseJacobian {
    pub n: usize,
    pub triplets: Vec<(usize, usize, f64)>,
}

impl SparseJacobian {
    pub fn new(n: usize) -> Self {
        Self { n, triplets: Vec::new() }
    }
    pub fn clear(&mut self) {
        self.triplets.clear();
    }
    pub fn add(&mut self, row: usize, col: usize, value: f64) {
        if value != 0.0 {
            self.triplets.push((row, col, value));
        }
    }
    pub fn from_dense(m: &DMatrix<f64>) -> Self {
        let mut out = Self::new(m.nrows());
        for r in 0..m.nrows() {
            for c in 0..m.ncols() {
                out.add(r, c, m[(r, c)]);
            }
        }
        out
    }
    /// Entries sorted by (row, column) with duplicates summed.
    pub fn summed(&self) -> Vec<(usize, usize, f64)> {
        let mut t = self.triplets.clone();
        t.sort_unstable_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let mut out: Vec<(usize, usize, f64)> = Vec::with_capacity(t.len());
        for e in t {
            match out.last_mut() {
                Some(last) if last.0 == e.0 && last.1 == e.1 => last.2 += e.2,
                _ => out.push(e),
            }
        }
        out
    }
    pub fn to_dense(&self) -> DMatrix<f64> {
        let mut m = DMatrix::zeros(self.n, self.n);
        for (r, c, v) in &self.triplets {
            m[(*r, *c)] += v;
        }
        m
    }
}

/// Symbolic LU analyses by sparsity pattern, shared by every solve.
static SYMBOLIC: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<u64, std::sync::Arc<faer::sparse::linalg::solvers::SymbolicLu<usize>>>>> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// A factorised, row-equilibrated Jacobian kept between Newton iterations
/// and between steps: the modified Newton method. Building a Jacobian
/// costs residual evaluations; while the iteration keeps contracting with
/// full steps the old factorisation is as good as a new one.
pub struct JacobianCache {
    lu: Factor,
    row_scale: Vec<f64>,
    /// Column equilibration: a sparse factorisation pivots for fill as
    /// much as for size, so columns of very different magnitude must be
    /// balanced beforehand. `δ = col_scale ∘ δ_scaled`.
    col_scale: Vec<f64>,
    /// How many iterations this factorisation has served.
    pub uses: usize,
}

/// Dense below `SPARSE_FROM` unknowns (partial pivoting on the whole row
/// is worth its n³ there and is what stiff small islands were tuned on);
/// sparse above it. `SIM_SPARSE_FROM` overrides the threshold.
enum Factor {
    Dense(nalgebra::LU<f64, nalgebra::Dyn, nalgebra::Dyn>),
    Sparse(faer::sparse::linalg::solvers::Lu<usize, f64>),
}

const SPARSE_FROM: usize = 256;

fn sparse_from() -> usize {
    std::env::var("SIM_SPARSE_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(SPARSE_FROM)
}

impl JacobianCache {
    /// Row-equilibrate and factorise; `None` when the matrix is singular.
    fn factorise(jacobian: &SparseJacobian) -> Option<Self> {
        let n = jacobian.n;
        let entries = jacobian.summed();
        if n < sparse_from() {
            let mut row_scale = vec![0.0_f64; n];
            for (r, _, v) in &entries {
                row_scale[*r] = row_scale[*r].max(v.abs());
            }
            for s in row_scale.iter_mut() {
                *s = if *s > 0.0 { 1.0 / *s } else { 1.0 };
            }
            let mut dense = DMatrix::zeros(n, n);
            for (r, c, v) in &entries {
                dense[(*r, *c)] += v * row_scale[*r];
            }
            let lu = dense.lu();
            // Singular if any pivot vanishes; the pivots sit on the packed
            // factor's diagonal (building `u()` would copy n×n per look).
            if lu.lu_internal().diagonal().iter().any(|d| *d == 0.0) {
                return None;
            }
            return Some(Self { lu: Factor::Dense(lu), row_scale, col_scale: vec![1.0; n], uses: 0 });
        }
        let mut row_scale = vec![0.0_f64; n];
        for (r, _, v) in &entries {
            row_scale[*r] = row_scale[*r].max(v.abs());
        }
        for s in row_scale.iter_mut() {
            *s = if *s > 0.0 { 1.0 / *s } else { 1.0 };
        }
        let mut col_scale = vec![0.0_f64; n];
        for (r, c, v) in &entries {
            col_scale[*c] = col_scale[*c].max((v * row_scale[*r]).abs());
        }
        for s in col_scale.iter_mut() {
            *s = if *s > 0.0 { 1.0 / *s } else { 1.0 };
        }
        let triplets: Vec<faer::sparse::Triplet<usize, usize, f64>> = entries.iter().map(|(r, c, v)| faer::sparse::Triplet::new(*r, *c, v * row_scale[*r] * col_scale[*c])).collect();
        let matrix = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &triplets).ok()?;
        // The symbolic analysis (ordering, fill) depends only on the
        // pattern, which a step's Jacobian keeps from rebuild to rebuild:
        // memoise it by pattern.
        let mut key = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        n.hash(&mut key);
        for (r, c, _) in &entries {
            (r, c).hash(&mut key);
        }
        let key = key.finish();
        let symbolic = {
            let mut memo = SYMBOLIC.lock().unwrap_or_else(|p| p.into_inner());
            match memo.get(&key) {
                Some(sym) => sym.clone(),
                None => {
                    let sym = std::sync::Arc::new(faer::sparse::linalg::solvers::SymbolicLu::try_new(matrix.symbolic()).ok()?);
                    if memo.len() > 64 {
                        memo.clear();
                    }
                    memo.insert(key, sym.clone());
                    sym
                }
            }
        };
        let lu = faer::sparse::linalg::solvers::Lu::try_new_with_symbolic((*symbolic).clone(), matrix.as_ref()).ok()?;
        Some(Self { lu: Factor::Sparse(lu), row_scale, col_scale, uses: 0 })
    }
    fn solve(&self, rhs: &[f64]) -> Option<Vec<f64>> {
        let n = rhs.len();
        let out: Vec<f64> = match &self.lu {
            Factor::Dense(lu) => {
                let b = nalgebra::DVector::from_column_slice(rhs);
                let x = lu.solve(&b)?;
                (0..n).map(|i| x[i]).collect()
            }
            Factor::Sparse(lu) => {
                use faer::linalg::solvers::Solve;
                let b = faer::Mat::<f64>::from_fn(n, 1, |i, _| rhs[i]);
                let x = lu.solve(&b);
                (0..n).map(|i| x[(i, 0)] * self.col_scale[i]).collect()
            }
        };
        out.iter().all(|v| v.is_finite()).then_some(out)
    }
}

pub fn solve_newton_scaled<F, J>(
    unknowns: &mut [f64],
    config: NewtonConfig,
    residual: F,
    mut jacobian_at: J,
    step_scale: &dyn Fn(usize, f64) -> f64,
) -> Result<SolveDiagnostics, SolveError>
where
    F: Fn(&[f64], &mut [f64]),
    J: FnMut(&mut [f64], &[f64], &mut DMatrix<f64>),
{
    let n = unknowns.len();
    let mut dense = DMatrix::zeros(n, n);
    solve_newton_cached(unknowns, config, residual, |x, r, sparse| {
        dense.fill(0.0);
        jacobian_at(x, r, &mut dense);
        *sparse = SparseJacobian::from_dense(&dense);
    }, step_scale, &mut None)
}

/// Row-equilibrated Newton with a reusable sparse factorisation. `cache`
/// carries the last Jacobian in and out; pass `None` to start fresh. A
/// stale factorisation is dropped the moment it stops paying: a line
/// search that finds nothing, a singular solve, or a step that fails to
/// halve the residual. A fresh one behaves exactly as plain Newton.
pub fn solve_newton_cached<F, J>(
    unknowns: &mut [f64],
    config: NewtonConfig,
    residual: F,
    mut jacobian_at: J,
    step_scale: &dyn Fn(usize, f64) -> f64,
    cache: &mut Option<JacobianCache>,
) -> Result<SolveDiagnostics, SolveError>
where
    F: Fn(&[f64], &mut [f64]),
    J: FnMut(&mut [f64], &[f64], &mut SparseJacobian),
{
    let n = unknowns.len();
    profile::NEWTON.count(1);
    let mut r = vec![0.0; n];
    let mut candidate_r = vec![0.0; n];
    let mut reductions = 0;
    let mut jacobian = SparseJacobian::new(n);

    profile::RESIDUAL.time(|| residual(unknowns, &mut r));
    finite(&r)?;
    // Rows carry different units and magnitudes (a torque balance in
    // nano-newton-metres beside a rate row in rad/s). Every norm below is
    // taken on rows scaled by their Jacobian row magnitude, which makes the
    // convergence test say "no unknown needs to move" in each row's own
    // terms rather than comparing raw residuals across rows.
    let scaled_norm = |values: &[f64], scale: &[f64]| values.iter().zip(scale).fold(0.0_f64, |m, (v, s)| m.max((v * s).abs()));

    let mut best_step = f64::INFINITY;
    let mut stalled = 0usize;
    let mut failed_searches = 0usize;
    let mut iteration = 0usize;
    let mut stale_tail = 0usize;
    loop {
        let fresh = cache.is_none();
        profile::ITERATIONS.count(1);
        if fresh {
            profile::FRESH.count(1);
            jacobian.clear();
            jacobian.n = n;
            profile::JACOBIAN.time(|| jacobian_at(unknowns, &r, &mut jacobian));
            if jacobian.triplets.iter().any(|(_, _, v)| !v.is_finite()) {
                return Err(SolveError::NonFinite);
            }
            match profile::FACTORISE.time(|| JacobianCache::factorise(&jacobian)) {
                Some(factor) => *cache = Some(factor),
                None => {
                    if trace_enabled() {
                        let entries = jacobian.summed();
                        let zero_rows: Vec<usize> = (0..n).filter(|r| !entries.iter().any(|(er, _, _)| er == r)).collect();
                        let zero_cols: Vec<usize> = (0..n).filter(|c| !entries.iter().any(|(_, ec, _)| ec == c)).collect();
                        eprintln!("newton singular: zero rows {zero_rows:?}, zero columns {zero_cols:?}");
                    }
                    return Err(SolveError::Singular { iteration });
                }
            }
        }
        let factor = cache.as_mut().expect("a factorisation");
        factor.uses += 1;
        let row_scale = factor.row_scale.clone();
        // Convergence is decided on the Newton correction below, per unknown
        // and on its own scale; only an exactly vanishing residual short-circuits.
        let norm = scaled_norm(&r, &row_scale);
        if norm == 0.0 {
            return Ok(SolveDiagnostics { iterations: iteration, residual_norm: 0.0, line_search_reductions: reductions });
        }
        if iteration >= config.max_iterations {
            if trace_enabled() {
                let mut rows: Vec<(usize, f64)> = (0..n).map(|i| (i, (r[i] * row_scale[i]).abs())).collect();
                rows.sort_by(|a, b| b.1.total_cmp(&a.1));
                eprintln!("newton gave up: worst scaled rows {:?}", &rows[..n.min(5)]);
                eprintln!("  raw residual at those rows {:?}", rows.iter().take(5).map(|(i, _)| (*i, r[*i])).collect::<Vec<_>>());
            }
            *cache = None;
            return Err(SolveError::NotConverged { iterations: iteration, residual: infinity_norm(&r) });
        }

        let rhs: Vec<f64> = (0..n).map(|row| -r[row] * row_scale[row]).collect();
        let Some(delta) = profile::SOLVE.time(|| factor.solve(&rhs)) else {
            if !fresh {
                // A stale factorisation went bad: rebuild and retry.
                *cache = None;
                continue;
            }
            *cache = None;
            return Err(SolveError::Singular { iteration });
        };

        // A step that would not move any unknown on its own scale means the
        // residual has reached its floor: take it and stop.
        let negligible = (0..n).all(|i| {
            let d = delta[i].abs();
            // The second bound is the value's own floating-point resolution,
            // which a finite-difference Jacobian cannot beat.
            d <= config.relative_tolerance * step_scale(i, unknowns[i]) || d <= 1.0e-13 * (1.0 + unknowns[i].abs())
        });
        if trace_enabled() {
            eprintln!("newton it {iteration}{}: scaled |r| {norm:.3e}, max|δ| {:.3e}, negligible {negligible}, u {:?}", if fresh { "" } else { " (reused J)" }, delta.iter().fold(0.0_f64, |m, v| m.max(v.abs())), &unknowns[..unknowns.len().min(9)]);
        }
        // Stagnation at the noise floor: the correction has stopped
        // shrinking while the row-scaled residual is already below the
        // absolute tolerance. A finite-difference Jacobian cannot do better;
        // this is convergence, not failure.
        let step_size = delta.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        if step_size < 0.5 * best_step {
            best_step = step_size;
            stalled = 0;
        } else {
            stalled += 1;
        }
        // "Negligible" on a 100× looser scale: what a stalled iteration
        // must still satisfy for the stall to count as convergence.
        let loosely_negligible = (0..n).all(|i| {
            let d = delta[i].abs();
            d <= 100.0 * config.relative_tolerance * step_scale(i, unknowns[i]) || d <= 1.0e-13 * (1.0 + unknowns[i].abs())
        });
        let at_floor = stalled >= 3 && (norm <= config.absolute_tolerance || loosely_negligible);
        // With a reused Jacobian the correction only bounds the error to
        // within the contraction factor, not quadratically; ask for a
        // hundredfold tighter correction before believing it, at the price
        // of a few more cheap iterations, and rebuild if that drags on.
        let tight = fresh || (0..n).all(|i| {
            let d = delta[i].abs();
            d <= 0.01 * config.relative_tolerance * step_scale(i, unknowns[i]) || d <= 1.0e-13 * (1.0 + unknowns[i].abs())
        });
        if (negligible && tight) || at_floor {
            for index in 0..n {
                unknowns[index] += delta[index];
            }
            profile::RESIDUAL.time(|| residual(unknowns, &mut r));
            finite(&r)?;
            return Ok(SolveDiagnostics { iterations: iteration + 1, residual_norm: infinity_norm(&r), line_search_reductions: reductions });
        }
        if negligible {
            // Stale and merely negligible: take the step and keep going;
            // after a few such steps a fresh Jacobian settles it.
            for index in 0..n {
                unknowns[index] += delta[index];
            }
            profile::RESIDUAL.time(|| residual(unknowns, &mut r));
            finite(&r)?;
            stale_tail += 1;
            if stale_tail >= 4 {
                *cache = None;
            }
            iteration += 1;
            continue;
        }

        let old = unknowns.to_vec();
        let old_norm = norm;
        // Full step when it helps; otherwise scan the halvings and take the
        // best, which is what a nonsmooth row (a complementarity function at
        // its kink) needs to make progress instead of crawling.
        let mut alpha = 1.0;
        let mut best: Option<(f64, f64, Vec<f64>)> = None;
        let mut retry_fresh = false;
        loop {
            for index in 0..n {
                unknowns[index] = old[index] + alpha * delta[index];
            }
            profile::RESIDUAL.time(|| residual(unknowns, &mut candidate_r));
            finite(&candidate_r)?;
            let candidate_norm = scaled_norm(&candidate_r, &row_scale);
            if alpha == 1.0 && candidate_norm < old_norm {
                failed_searches = 0;
                r.copy_from_slice(&candidate_r);
                // A reused factorisation earns its keep by halving the
                // residual; otherwise the next iteration rebuilds.
                if !fresh && candidate_norm > 0.5 * old_norm {
                    *cache = None;
                }
                break;
            }
            if best.as_ref().is_none_or(|(norm, _, _)| candidate_norm < *norm) {
                best = Some((candidate_norm, alpha, candidate_r.clone()));
            }
            if alpha <= config.min_line_search {
                let (best_norm, best_alpha, best_r) = best.take().unwrap();
                if !fresh {
                    // The stale Jacobian led nowhere: back to the start of
                    // this iteration with a fresh one, no failure counted.
                    unknowns.copy_from_slice(&old);
                    *cache = None;
                    retry_fresh = true;
                    break;
                }
                // No step length helps: take the least bad one, but not
                // for long — three such iterations in a row mean the
                // iteration is lost, and the caller (a shorter time step, a
                // nonsmooth element's branch) has better options.
                failed_searches = if best_norm < old_norm { 0 } else { failed_searches + 1 };
                if failed_searches >= 3 {
                    if trace_enabled() {
                        let mut rows: Vec<(usize, f64)> = (0..n).map(|i| (i, (r[i] * row_scale[i]).abs())).collect();
                        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
                        let mut steps: Vec<(usize, f64)> = (0..n).map(|i| (i, delta[i].abs())).collect();
                        steps.sort_by(|a, b| b.1.total_cmp(&a.1));
                        eprintln!("newton line search lost: worst scaled rows {:?} raw {:?} largest steps {:?}", &rows[..n.min(4)], rows.iter().take(4).map(|(i, _)| (*i, r[*i])).collect::<Vec<_>>(), &steps[..n.min(4)]);
                    }
                    *cache = None;
                    return Err(SolveError::NotConverged { iterations: iteration + 1, residual: infinity_norm(&r) });
                }
                for index in 0..n {
                    unknowns[index] = old[index] + best_alpha * delta[index];
                }
                r.copy_from_slice(&best_r);
                // A partial step with a fresh Jacobian is not a reason to
                // keep it around.
                *cache = None;
                break;
            }
            alpha *= 0.5;
            reductions += 1;
        }
        if !retry_fresh {
            iteration += 1;
        }
    }
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

pub mod profile;

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

/// `SIM_NEWTON_TRACE=1` prints every Newton iteration, step header, branch
/// restart and event crossing to stderr — the first thing to reach for
/// when a step will not converge.
fn trace_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("SIM_NEWTON_TRACE").is_some())
}

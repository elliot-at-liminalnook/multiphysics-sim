//! Small, deterministic nonlinear solver used by the first coupling island.

mod coloring;
pub use coloring::{BlockDiagonalColoring, solve_newton_numeric_colored, solve_newton_numeric_colored_scaled_audited};

use nalgebra::DMatrix;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NewtonConfig {
    /// Absolute floor for final raw residuals, in each equation's supplied
    /// units. Also nominates scaled stagnation candidates for verification.
    pub absolute_tolerance: f64,
    /// Correction tolerance and required residual reduction relative to the
    /// initial residual of each row. Linear-system scaling cannot relax it.
    pub relative_tolerance: f64,
    pub max_iterations: usize,
    pub min_line_search: f64,
    /// Experimental: stop a decreasing backtrack after three successive worse
    /// probes, away from the scaled numerical floor. This chooses the best
    /// observed trial, not necessarily the best of the exhaustive halving grid.
    /// Raw residual/correction acceptance checks are unchanged. Off by default.
    #[serde(default)]
    pub guarded_backtracking: bool,
    /// Experimental domain-aware line search: a nonfinite trial is rejected
    /// and shortened. Initial residuals and Jacobian probes still fail closed.
    /// Useful when trial coordinates can leave a valid local mechanism chart.
    #[serde(default)]
    pub reject_nonfinite_trials: bool,
    /// Experimental: reserve the final two iterations for fresh Jacobians.
    /// A contracting modified-Newton tail must not exhaust the budget solely
    /// because cached corrections require a stricter convergence estimate.
    /// The iteration cap and raw-residual/correction tolerances are unchanged.
    #[serde(default, skip_serializing_if = "is_false")]
    pub refresh_before_iteration_limit: bool,
}

impl Default for NewtonConfig {
    fn default() -> Self {
        Self {
            absolute_tolerance: 1.0e-10,
            relative_tolerance: 1.0e-8,
            max_iterations: 18,
            min_line_search: 1.0 / 256.0,
            guarded_backtracking: false,
            reject_nonfinite_trials: false,
            refresh_before_iteration_limit: false,
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
    solve_newton_numeric_cached(unknowns, config, residual, &mut None)
}

/// Numerical forward-difference Newton with a reusable correction matrix.
/// Residuals and acceptance bounds remain current. Clone the cache before a
/// speculative solve; commit it only with the accepted physical state. Clear it
/// when changing equation definitions, coordinates or discrete modes.
pub fn solve_newton_numeric_cached<F>(
    unknowns: &mut [f64], config: NewtonConfig, residual: F,
    cache: &mut Option<JacobianCache>,
) -> Result<SolveDiagnostics, SolveError>
where F: Fn(&[f64], &mut [f64]),
{
    solve_newton_numeric_cached_audited(unknowns, config, residual, cache, None)
}

/// The same numerical solve with optional bounded iteration diagnostics.
/// Recording an audit does not change residual calls or acceptance decisions.
pub fn solve_newton_numeric_cached_audited<F>(
    unknowns: &mut [f64], config: NewtonConfig, residual: F,
    cache: &mut Option<JacobianCache>, audit: Option<&mut NewtonAudit>,
) -> Result<SolveDiagnostics, SolveError>
where F: Fn(&[f64], &mut [f64]),
{
    solve_newton_numeric_scaled_cached_audited(unknowns,config,residual,&|_,value|1.0+value.abs(),cache,audit)
}

/// Numerical Newton in caller-selected coordinates. `step_scale` expresses
/// a correction scale in those coordinates; residual units and acceptance
/// bounds are unchanged. For an affine coordinate x=x0+h*u, a physical state
/// scale s(x) transforms to s(x)/h for corrections in u.
pub fn solve_newton_numeric_scaled_cached_audited<F>(
    unknowns: &mut [f64], config: NewtonConfig, residual: F,
    step_scale: &dyn Fn(usize,f64)->f64,
    cache: &mut Option<JacobianCache>, audit: Option<&mut NewtonAudit>,
) -> Result<SolveDiagnostics, SolveError>
where F: Fn(&[f64], &mut [f64]),
{
    let n = unknowns.len();
    let mut perturbed_r = vec![0.0; n];
    solve_newton_cached_audited(unknowns, config, &residual, |x, base, jacobian| {
        for column in 0..n {
            let original = x[column];
            // Keep the reference perturbation above nested finite-difference
            // noise; this helper changes cache lifetime, not derivative steps.
            let epsilon = 1.0e-6 * (1.0 + original.abs());
            x[column] = original + epsilon;
            residual(x, &mut perturbed_r);
            x[column] = original;
            for row in 0..n {
                jacobian.add(row, column, (perturbed_r[row] - base[row]) / epsilon);
            }
        }
    }, step_scale, cache, audit)
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
#[derive(Clone)]
pub struct JacobianCache {
    // Immutable factor storage is shared by transactional cache snapshots.
    lu: std::sync::Arc<Factor>,
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

/// Convert unique row-major entries to CSC without sorting them a second time.
/// `summed` retains the existing duplicate-addition order. Scattering its rows
/// into each column produces the same sorted indices and scaled values as
/// Faer's general triplet constructor, including explicit and signed zeros.
fn scaled_column_matrix(n: usize, entries: &[(usize, usize, f64)], row_scale: &[f64], col_scale: &[f64])
    -> faer::sparse::SparseColMat<usize, f64> {
    let mut col_ptr = vec![0; n + 1];
    for &(_, c, _) in entries { col_ptr[c + 1] += 1; }
    for c in 0..n { col_ptr[c + 1] += col_ptr[c]; }
    let mut next = col_ptr[..n].to_vec();
    let mut row_idx = vec![0; entries.len()];
    let mut values = vec![0.0; entries.len()];
    for &(r, c, value) in entries {
        let at = next[c];
        next[c] += 1;
        row_idx[at] = r;
        values[at] = value * row_scale[r] * col_scale[c];
    }
    let pattern = faer::sparse::SymbolicSparseColMat::new_checked(n, n, col_ptr, None, row_idx);
    faer::sparse::SparseColMat::new(pattern, values)
}

impl JacobianCache {
    /// Row-equilibrate and factorise; `None` when the matrix is singular.
    fn factorise(jacobian: &SparseJacobian) -> Option<Self> {
        let n = jacobian.n;
        let entries = profile::FACTOR_SUM.time(|| jacobian.summed());
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
            return Some(Self { lu: std::sync::Arc::new(Factor::Dense(lu)), row_scale, col_scale: vec![1.0; n], uses: 0 });
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
        let matrix = profile::FACTOR_MATRIX.time(|| scaled_column_matrix(n, &entries, &row_scale, &col_scale));
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
        let symbolic = profile::FACTOR_SYMBOLIC.time(|| {
            let mut memo = SYMBOLIC.lock().unwrap_or_else(|p| p.into_inner());
            Some(match memo.get(&key) {
                Some(sym) => sym.clone(),
                None => {
                    let sym = std::sync::Arc::new(profile::FACTOR_SYMBOLIC_BUILD.time(|| faer::sparse::linalg::solvers::SymbolicLu::try_new(matrix.symbolic())).ok()?);
                    if memo.len() > 64 {
                        memo.clear();
                    }
                    memo.insert(key, sym.clone());
                    sym
                }
            })
        })?;
        let lu = profile::FACTOR_NUMERIC.time(|| faer::sparse::linalg::solvers::Lu::try_new_with_symbolic((*symbolic).clone(), matrix.as_ref())).ok()?;
        Some(Self { lu: std::sync::Arc::new(Factor::Sparse(lu)), row_scale, col_scale, uses: 0 })
    }
    fn solve(&self, rhs: &[f64]) -> Option<Vec<f64>> {
        let n = rhs.len();
        let out: Vec<f64> = match self.lu.as_ref() {
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
/// stale factorisation is dropped after a nondecreasing full trial, a singular
/// solve, or a step that fails to halve the residual. A fresh factorisation
/// retains the complete backtracking search. Residual evaluations must describe
/// the same equations throughout a solve.
pub fn solve_newton_cached<F, J>(
    unknowns: &mut [f64],
    config: NewtonConfig,
    residual: F,
    jacobian_at: J,
    step_scale: &dyn Fn(usize, f64) -> f64,
    cache: &mut Option<JacobianCache>,
) -> Result<SolveDiagnostics, SolveError>
where
    F: Fn(&[f64], &mut [f64]),
    J: FnMut(&mut [f64], &[f64], &mut SparseJacobian),
{
    solve_newton_cached_audited(unknowns, config, residual, jacobian_at, step_scale, cache, None)
}

/// Optional bounded diagnostics; the default solver does not collect them.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NewtonAudit {
    pub iterations: Vec<NewtonIteration>,
    pub row_scale: Vec<f64>,
    /// Fixed raw-residual acceptance bounds, independent of Jacobian scaling.
    /// Empty in legacy audit records that did not enforce these bounds.
    #[serde(default)]
    pub residual_limits: Vec<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewtonIteration {
    pub iteration: usize,
    pub fresh_jacobian: bool,
    pub scaled_residual_norm: f64,
    pub largest_rows: Vec<(usize, f64, f64)>,
    pub decision: String,
    /// Absent before a linear correction exists, and in legacy audit records.
    #[serde(default)]
    pub correction: Option<NewtonCorrectionAudit>,
    /// Finite backtracking trials, absent when no search was needed or in
    /// legacy captures. Observational only; never used to choose a step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_search: Option<NewtonLineSearchAudit>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NewtonLineSearchAudit {
    pub trials: Vec<NewtonLineSearchTrial>,
    /// None when a stale matrix was refreshed or the search failed.
    pub selected_alpha: Option<f64>,
    /// True only when the experimental guard omitted remaining halving probes.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bracketed: bool,
}
fn is_false(value: &bool) -> bool { !*value }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewtonLineSearchTrial {
    pub alpha: f64,
    /// Uses the same fixed Jacobian row scale as the iteration entry.
    pub scaled_residual_norm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewtonCorrectionAudit {
    /// (unknown index, signed correction, negligible bound, |correction|/bound).
    /// Largest normalized corrections first; bounded to eight entries.
    pub largest_unknowns: Vec<(usize, f64, f64, f64)>,
    pub negligible: bool,
    pub tight: bool,
    pub at_floor: bool,
}

pub fn solve_newton_cached_audited<F, J>(
    unknowns: &mut [f64],
    config: NewtonConfig,
    residual: F,
    mut jacobian_at: J,
    step_scale: &dyn Fn(usize, f64) -> f64,
    cache: &mut Option<JacobianCache>,
    mut audit: Option<&mut NewtonAudit>,
) -> Result<SolveDiagnostics, SolveError>
where
    F: Fn(&[f64], &mut [f64]),
    J: FnMut(&mut [f64], &[f64], &mut SparseJacobian),
{
    macro_rules! decision {
        ($message:expr) => {
            if let Some(record) = audit.as_deref_mut().and_then(|a| a.iterations.last_mut()) {
                record.decision = $message.into();
            }
        };
    }
    let n = unknowns.len();
    if cache.as_ref().is_some_and(|c| c.row_scale.len() != n) {
        *cache = None;
    }
    profile::NEWTON.count(1);
    let mut r = vec![0.0; n];
    let mut candidate_r = vec![0.0; n];
    let mut reductions = 0;
    let mut jacobian = SparseJacobian::new(n);

    profile::RESIDUAL.time(|| residual(unknowns, &mut r));
    finite(&r)?;
    // Acceptance must not become easier when a probe across a discontinuity
    // inflates the Jacobian. Require actual residual reduction against fixed
    // per-row bounds from the beginning of this solve, independently of the
    // row scaling used to condition the linear system.
    let residual_limits: Vec<_> = r.iter().map(|v|
        config.absolute_tolerance + config.relative_tolerance * v.abs()
    ).collect();
    if let Some(audit) = audit.as_deref_mut() {
        audit.residual_limits.clone_from(&residual_limits);
    }
    // Rows carry different units and magnitudes (a torque balance in
    // nano-newton-metres beside a rate row in rad/s). Every norm below is
    // taken on rows scaled by their Jacobian row magnitude, which makes the
    // correction test say "no unknown needs to move" in each row's own
    // terms. Final acceptance also checks the fixed raw-residual bounds above.
    let scaled_norm = |values: &[f64], scale: &[f64]| values.iter().zip(scale).fold(0.0_f64, |m, (v, s)| m.max((v * s).abs()));

    let mut best_step = f64::INFINITY;
    let mut stalled = 0usize;
    let mut failed_searches = 0usize;
    let mut iteration = 0usize;
    let mut stale_tail = 0usize;
    let mut rejected_acceptances = 0usize;
    let mut rejected_norm = f64::INFINITY;
    loop {
        if config.refresh_before_iteration_limit
            && iteration < config.max_iterations
            && iteration.saturating_add(2) >= config.max_iterations
        {
            *cache = None;
        }
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
        if let Some(audit) = audit.as_deref_mut() {
            audit.row_scale.clone_from(&row_scale);
            if audit.iterations.len() < config.max_iterations.saturating_mul(4).saturating_add(16) {
                let mut rows: Vec<_> = (0..n).map(|i| (i, r[i], (r[i]*row_scale[i]).abs())).collect();
                rows.sort_by(|a,b| b.2.total_cmp(&a.2));
                rows.truncate(8);
                audit.iterations.push(NewtonIteration { iteration, fresh_jacobian:fresh,
                    scaled_residual_norm:norm, largest_rows:rows, decision:"correction".into(), correction:None, line_search:None });
            }
        }
        if r.iter().all(|value| *value == 0.0) {
            decision!("zero_residual_accept");
            return Ok(SolveDiagnostics { iterations: iteration, residual_norm: 0.0, line_search_reductions: reductions });
        }
        if iteration >= config.max_iterations {
            decision!("iteration_limit");
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
                decision!("stale_linear_solve_refresh");
                // A stale factorisation went bad: rebuild and retry.
                *cache = None;
                continue;
            }
            *cache = None;
            return Err(SolveError::Singular { iteration });
        };

        // A small correction nominates a convergence candidate; it does not
        // prove that the actual residual has reached its requested tolerance.
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
        // absolute tolerance. This still requires raw-residual verification:
        // a difference across a jump can produce a spurious apparent floor.
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
        if let Some(record) = audit.as_deref_mut().and_then(|a| a.iterations.last_mut()) {
            let mut largest_unknowns: Vec<_> = (0..n).map(|i| {
                let bound = (config.relative_tolerance * step_scale(i, unknowns[i]))
                    .max(1.0e-13 * (1.0 + unknowns[i].abs()));
                (i, delta[i], bound, delta[i].abs() / bound)
            }).collect();
            largest_unknowns.sort_by(|a,b| b.3.total_cmp(&a.3));
            largest_unknowns.truncate(8);
            record.correction = Some(NewtonCorrectionAudit { largest_unknowns, negligible, tight, at_floor });
        }
        if (negligible && tight) || at_floor {
            decision!(if at_floor { "noise_floor_accept" } else { "correction_accept" });
            for index in 0..n {
                unknowns[index] += delta[index];
            }
            profile::RESIDUAL.time(|| residual(unknowns, &mut r));
            finite(&r)?;
            if !r.iter().zip(&residual_limits).all(|(value, limit)| value.abs() <= *limit) {
                decision!("residual_acceptance_refresh");
                *cache = None;
                let actual_norm = r.iter().zip(&residual_limits)
                    .fold(0.0_f64, |m, (v, limit)| m.max(v.abs() / limit));
                rejected_acceptances = if actual_norm < 0.5 * rejected_norm { 0 } else { rejected_acceptances + 1 };
                rejected_norm = actual_norm;
                // A fresh matrix gets a chance before rejecting the trial.
                // Repeated unproductive refreshes at an apparent noise floor
                // belong to timestep/branch recovery, not a long false tail.
                if rejected_acceptances >= 3 {
                    decision!("residual_acceptance_failure");
                    return Err(SolveError::NotConverged { iterations: iteration + 1, residual: infinity_norm(&r) });
                }
                iteration += 1;
                continue;
            }
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
                decision!("stale_tail_refresh");
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
        let mut previous_trial_norm = f64::INFINITY;
        let mut increasing_trials = 0usize;
        if let Some(record) = audit.as_deref_mut().and_then(|a| a.iterations.last_mut()) {
            record.line_search = Some(NewtonLineSearchAudit::default());
        }
        macro_rules! selected_alpha {
            ($alpha:expr) => {
                if let Some(search) = audit.as_deref_mut()
                    .and_then(|a| a.iterations.last_mut()).and_then(|r| r.line_search.as_mut()) {
                    search.selected_alpha = Some($alpha);
                }
            };
        }
        loop {
            for index in 0..n {
                unknowns[index] = old[index] + alpha * delta[index];
            }
            profile::RESIDUAL.time(|| residual(unknowns, &mut candidate_r));
            let candidate_norm = match finite(&candidate_r) {
                Ok(()) => scaled_norm(&candidate_r, &row_scale),
                Err(e) if !config.reject_nonfinite_trials => return Err(e),
                Err(_) => f64::INFINITY,
            };
            if let Some(search) = audit.as_deref_mut()
                .and_then(|a| a.iterations.last_mut()).and_then(|r| r.line_search.as_mut()) {
                search.trials.push(NewtonLineSearchTrial { alpha, scaled_residual_norm: candidate_norm });
            }
            if alpha == 1.0 && candidate_norm < old_norm {
                selected_alpha!(alpha);
                failed_searches = 0;
                r.copy_from_slice(&candidate_r);
                // A reused factorisation earns its keep by halving the
                // residual; otherwise the next iteration rebuilds.
                if !fresh && candidate_norm > 0.5 * old_norm {
                    decision!("poor_contraction_refresh");
                    *cache = None;
                }
                break;
            }
            if !fresh {
                profile::STALE_FULL_REFRESH.count(1);
                decision!("stale_line_search_refresh");
                // A stale matrix is eligible only for a decreasing full step.
                // Backtracked candidates would all be discarded before a fresh
                // retry, so restore the original point/residual immediately.
                // The full candidate has still passed the finite-value check.
                unknowns.copy_from_slice(&old);
                *cache = None;
                retry_fresh = true;
                break;
            }
            if candidate_norm.is_finite() && best.as_ref().is_none_or(|(norm, _, _)| candidate_norm < *norm) {
                best = Some((candidate_norm, alpha, candidate_r.clone()));
            }
            if config.guarded_backtracking {
                increasing_trials = if candidate_norm > previous_trial_norm { increasing_trials + 1 } else { 0 };
                previous_trial_norm = candidate_norm;
            }
            // Heuristic only: nonmonotone tails can improve again. Preserve the
            // exhaustive reference and avoid early stops near the scaled floor.
            let bracketed = config.guarded_backtracking && alpha > config.min_line_search
                && old_norm > 100.0 * config.absolute_tolerance
                && increasing_trials >= 3 && best.as_ref().is_some_and(|b| b.0 < old_norm);
            if bracketed {
                if let Some(search) = audit.as_deref_mut()
                    .and_then(|a| a.iterations.last_mut()).and_then(|r| r.line_search.as_mut()) {
                    search.bracketed = true;
                }
            }
            if alpha <= config.min_line_search || bracketed {
                let Some((best_norm, best_alpha, best_r)) = best.take() else {
                    decision!("no_finite_line_search_trial");
                    unknowns.copy_from_slice(&old);
                    *cache = None;
                    return Err(SolveError::NonFinite);
                };
                // No step length helps: take the least bad one, but not
                // for long — three such iterations in a row mean the
                // iteration is lost, and the caller (a shorter time step, a
                // nonsmooth element's branch) has better options.
                failed_searches = if best_norm < old_norm { 0 } else { failed_searches + 1 };
                if failed_searches >= 3 {
                    decision!("line_search_failure");
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
                selected_alpha!(best_alpha);
                decision!("partial_step_refresh");
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
    fn direct_csc_matches_triplet_constructor_bits_for_duplicates_zeros_and_empty_columns() {
        for n in [0, 1, 7, 300] {
            let mut jac = SparseJacobian::new(n);
            for i in 0..n {
                let c = if n > 1 { i % (n-1) } else {0};
                jac.triplets.extend([(i,c,1e16),(i,c,1.0),(i,c,-1e16)]);
                jac.triplets.push(((i*37+1)%n,c,1e-300));
                jac.triplets.push(((i*13+2)%n,c,-0.0));
            }
            jac.triplets.reverse();
            let entries=jac.summed();
            let row:Vec<_>=(0..n).map(|i|10.0_f64.powi((i%21) as i32-10)).collect();
            let col:Vec<_>=(0..n).map(|i|10.0_f64.powi(10-(i%21) as i32)).collect();
            let expected=faer::sparse::SparseColMat::<usize,f64>::try_new_from_triplets(n,n,
                &entries.iter().map(|&(r,c,v)|faer::sparse::Triplet::new(r,c,v*row[r]*col[c])).collect::<Vec<_>>()).unwrap();
            let actual=scaled_column_matrix(n,&entries,&row,&col);
            assert_eq!(actual.symbolic().col_ptr(),expected.symbolic().col_ptr());
            assert_eq!(actual.symbolic().row_idx(),expected.symbolic().row_idx());
            assert_eq!(actual.val().iter().map(|v|v.to_bits()).collect::<Vec<_>>(),
                expected.val().iter().map(|v|v.to_bits()).collect::<Vec<_>>());
        }
    }

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

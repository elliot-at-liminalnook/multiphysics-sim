//! Domain-agnostic time integration.
//!
//! A [`System`] is anything that can say how far a proposed `(state, rate)`
//! pair is from satisfying its equations. That single residual form covers
//! explicit ODEs, index-1 DAEs with algebraic unknowns, and piecewise-smooth
//! models that switch mode at [`System::jump`]. A [`Simulation`] owns one
//! system, an [`Integrator`], the current state and a [`Trace`], and is the
//! only thing a scenario needs to drive.
//!
//! ```
//! use sim_dynamics::{Integrator, Ode, Simulation};
//!
//! struct Oscillator;
//! impl Ode for Oscillator {
//!     fn dimension(&self) -> usize { 2 }
//!     fn derivative(&self, _t: f64, x: &[f64], dxdt: &mut [f64]) {
//!         dxdt[0] = x[1];
//!         dxdt[1] = -x[0];
//!     }
//!     fn energy(&self, _t: f64, x: &[f64]) -> Option<f64> {
//!         Some(0.5 * (x[0] * x[0] + x[1] * x[1]))
//!     }
//! }
//!
//! let mut sim = Simulation::new(Oscillator, Integrator::implicit_midpoint(), vec![1.0, 0.0]);
//! sim.run(10.0, 1.0e-3).unwrap();
//! assert!((sim.energy().unwrap() - 0.5).abs() < 1.0e-9);
//! ```

pub mod analysis;
pub mod jacobian;
pub mod linear;
pub mod report;

use jacobian::Sparsity;
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};
use sim_solve::{JacobianCache, NewtonConfig, SolveError, profile, solve_newton_cached};
pub use sim_solve::SparseJacobian;
use thiserror::Error;

/// Equations in implicit residual form `r(t, x, ẋ) = 0`.
///
/// Rows that carry a time derivative are differential; rows that ignore
/// `rate` are algebraic. Implicit integrators handle both. Explicit
/// integrators need [`System::derivative`], which [`Ode`] supplies.
pub trait System {
    fn dimension(&self) -> usize;

    fn residual(&self, t: f64, x: &[f64], rate: &[f64], residual: &mut [f64]);

    /// Explicit rate `ẋ = f(t, x)`; `false` when the system has algebraic rows.
    fn derivative(&self, _t: f64, _x: &[f64], _dxdt: &mut [f64]) -> bool {
        false
    }

    /// A stored or conserved quantity worth tracking alongside the state.
    fn energy(&self, _t: f64, _x: &[f64]) -> Option<f64> {
        None
    }

    /// Guard functions for hybrid behavior. An event fires on the step in
    /// which a guard goes from positive to non-positive; the step is then
    /// bisected to locate the crossing before [`System::jump`] is applied.
    fn guards(&self, _t: f64, _x: &[f64], _guards: &mut Vec<f64>) {}

    /// State reset and mode switch for the guard at `index`.
    fn jump(&mut self, _index: usize, _t: f64, _x: &mut [f64]) {}
    /// Alternative full-state starts for an implicit step that failed from
    /// the smooth predictor: the branches of the system's nonsmooth
    /// elements (stick where slip was assumed, and so on). Tried in order
    /// before the step is subdivided.
    fn branches(&self, _t: f64, _x: &[f64]) -> Vec<Vec<f64>> {
        Vec::new()
    }
    /// A step of size `h` is about to be attempted: draw its noise.
    fn begin_step(&self, _h: f64) {}
    /// Seed the system's noise generator.
    fn seed_noise(&mut self, _seed: u64) {}

    /// Unknowns whose rates never enter the residual (reactions, multipliers,
    /// node potentials). The implicit midpoint rule evaluates these at the
    /// end of the step rather than the midpoint, which removes the ±
    /// alternation index-1 DAEs otherwise show.
    fn algebraic(&self) -> Option<Vec<bool>> {
        None
    }

    /// Which residual rows each unknown can affect, when known. Implicit
    /// integrators then assemble finite-difference Jacobians in as many
    /// residual evaluations as the pattern's colouring needs.
    fn sparsity(&self) -> Option<Sparsity> {
        None
    }

    /// Analytic Jacobian of the residual at `(t, x, rate)`, as sparse
    /// parts: `∂r/∂x` and `∂r/∂ẋ` triplets. Return `false` to fall back to
    /// finite differences on the sparsity pattern.
    fn jacobian(&self, _t: f64, _x: &[f64], _rate: &[f64], _out: &mut JacobianParts) -> bool {
        false
    }
}

/// `∂r/∂x` and `∂r/∂ẋ` as summed triplets `(row, column, value)`.
#[derive(Debug, Clone, Default)]
pub struct JacobianParts {
    pub d_dx: Vec<(usize, usize, f64)>,
    pub d_drate: Vec<(usize, usize, f64)>,
}

impl JacobianParts {
    pub fn clear(&mut self) {
        self.d_dx.clear();
        self.d_drate.clear();
    }
    pub fn dx(&mut self, row: usize, col: usize, value: f64) {
        if value != 0.0 {
            self.d_dx.push((row, col, value));
        }
    }
    pub fn drate(&mut self, row: usize, col: usize, value: f64) {
        if value != 0.0 {
            self.d_drate.push((row, col, value));
        }
    }
    /// Dense copies, for analyses that want matrices.
    pub fn dense(&self, n: usize) -> (DMatrix<f64>, DMatrix<f64>) {
        let (mut a, mut e) = (DMatrix::zeros(n, n), DMatrix::zeros(n, n));
        for (r, c, v) in &self.d_dx {
            a[(*r, *c)] += v;
        }
        for (r, c, v) in &self.d_drate {
            e[(*r, *c)] += v;
        }
        (a, e)
    }
}

/// Explicit first-order form `ẋ = f(t, x)`. Every [`Ode`] is a [`System`].
pub trait Ode {
    fn dimension(&self) -> usize;
    fn derivative(&self, t: f64, x: &[f64], dxdt: &mut [f64]);
    fn energy(&self, _t: f64, _x: &[f64]) -> Option<f64> {
        None
    }
    fn guards(&self, _t: f64, _x: &[f64], _guards: &mut Vec<f64>) {}
    fn jump(&mut self, _index: usize, _t: f64, _x: &mut [f64]) {}
}

impl<T: Ode> System for T {
    fn dimension(&self) -> usize {
        Ode::dimension(self)
    }
    fn residual(&self, t: f64, x: &[f64], rate: &[f64], residual: &mut [f64]) {
        Ode::derivative(self, t, x, residual);
        for (r, rate) in residual.iter_mut().zip(rate) {
            *r = rate - *r;
        }
    }
    fn derivative(&self, t: f64, x: &[f64], dxdt: &mut [f64]) -> bool {
        Ode::derivative(self, t, x, dxdt);
        true
    }
    fn energy(&self, t: f64, x: &[f64]) -> Option<f64> {
        Ode::energy(self, t, x)
    }
    fn guards(&self, t: f64, x: &[f64], guards: &mut Vec<f64>) {
        Ode::guards(self, t, x, guards)
    }
    fn jump(&mut self, index: usize, t: f64, x: &mut [f64]) {
        Ode::jump(self, index, t, x)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Integrator {
    /// Second order, symplectic, A-stable. The default for anything stiff,
    /// constrained or conservative.
    ImplicitMidpoint(NewtonConfig),
    /// First order, L-stable: damps the modes it cannot resolve instead of
    /// letting them ring. For a stiff network whose fast modes are not of
    /// interest — the acoustics of a water column, say — and never for
    /// anything whose energy budget is the point.
    BackwardEuler(NewtonConfig),
    /// Classical fourth-order Runge–Kutta. Explicit systems only.
    Rk4,
}

impl Integrator {
    pub fn implicit_midpoint() -> Self {
        Self::ImplicitMidpoint(NewtonConfig::default())
    }
}

#[derive(Debug, Error)]
pub enum DynamicsError {
    #[error("integrator requires an explicit derivative but the system is implicit")]
    NotExplicit,
    #[error("initial state has {actual} entries but the system has dimension {expected}")]
    Dimension { expected: usize, actual: usize },
    #[error("step size must be positive and finite, got {0}")]
    InvalidStep(f64),
    #[error("state became non-finite at t={0}")]
    NonFinite(f64),
    #[error("at t={time}: {source}")]
    Solve { time: f64, source: SolveError },
}

/// One hybrid event that fired during a run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub time: f64,
    pub guard: usize,
}

/// Columnar record of a run: one row per recorded step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trace {
    pub time: Vec<f64>,
    pub state: Vec<Vec<f64>>,
    pub energy: Vec<f64>,
}

impl Trace {
    pub fn len(&self) -> usize {
        self.time.len()
    }

    pub fn is_empty(&self) -> bool {
        self.time.is_empty()
    }

    /// One state component over time.
    pub fn column(&self, index: usize) -> Vec<f64> {
        self.state.iter().map(|row| row[index]).collect()
    }

    /// An arbitrary derived signal over time.
    pub fn map(&self, f: impl Fn(f64, &[f64]) -> f64) -> Vec<f64> {
        self.time
            .iter()
            .zip(&self.state)
            .map(|(t, x)| f(*t, x))
            .collect()
    }

    /// Write the trace as CSV: `time`, one column per name, then `energy`.
    pub fn write_csv(&self, path: impl AsRef<std::path::Path>, names: &[&str]) -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        write!(file, "time")?;
        for name in names {
            write!(file, ",{name}")?;
        }
        writeln!(file, ",energy")?;
        for (k, t) in self.time.iter().enumerate() {
            write!(file, "{t}")?;
            for value in &self.state[k] {
                write!(file, ",{value}")?;
            }
            writeln!(file, ",{}", self.energy.get(k).copied().unwrap_or(f64::NAN))?;
        }
        Ok(())
    }

    /// Rows with `time >= start`.
    pub fn after(&self, start: f64) -> Trace {
        let from = self.time.partition_point(|t| *t < start);
        Trace {
            time: self.time[from..].to_vec(),
            state: self.state[from..].to_vec(),
            energy: self.energy[from..].to_vec(),
        }
    }

    fn push(&mut self, time: f64, state: &[f64], energy: Option<f64>) {
        self.time.push(time);
        self.state.push(state.to_vec());
        self.energy.push(energy.unwrap_or(f64::NAN));
    }
}

/// A resumable point of a [`Simulation`]: see `Simulation::snapshot`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub time: f64,
    pub state: Vec<f64>,
    pub previous_rate: Vec<f64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RunStats {
    pub steps: u64,
    pub max_newton_iterations: usize,
    pub events: u64,
    /// Implicit steps that had to be split because Newton did not converge.
    pub subdivided_steps: u64,
    /// Steps that converged only from a branch proposed by a nonsmooth element.
    pub branch_restarts: u64,
}

pub struct Simulation<S: System> {
    pub system: S,
    pub time: f64,
    pub state: Vec<f64>,
    pub integrator: Integrator,
    pub trace: Trace,
    pub events: Vec<Event>,
    pub stats: RunStats,
    /// Record every N-th step into the trace; `0` disables tracing.
    pub record_every: u64,
    /// Bisection tolerance on event time as a fraction of the step.
    pub event_tolerance: f64,
    halt_at_event: bool,
    /// Rate of the last accepted step, used as the predictor for implicit
    /// steps (a compiled DAE has no explicit derivative to predict from).
    previous_rate: Vec<f64>,
    sparsity: Sparsity,
    algebraic: Vec<bool>,
    guards_before: Vec<f64>,
    guards_after: Vec<f64>,
    /// The last step's factorised Jacobian with the `(h, θ)` it was built
    /// for; reused by the next step while Newton keeps contracting.
    newton_cache: Option<(f64, f64, JacobianCache)>,
}

impl<S: System> Simulation<S> {
    pub fn new(system: S, integrator: Integrator, initial: Vec<f64>) -> Self {
        let mut trace = Trace::default();
        let energy = system.energy(0.0, &initial);
        trace.push(0.0, &initial, energy);
        let dimension = initial.len();
        let sparsity = system.sparsity().unwrap_or_else(|| Sparsity::new((0..dimension).map(|_| (0..dimension).collect()).collect()));
        let algebraic = system.algebraic().unwrap_or_else(|| vec![false; dimension]);
        Self {
            sparsity,
            algebraic,
            system,
            time: 0.0,
            state: initial,
            integrator,
            trace,
            events: Vec::new(),
            stats: RunStats::default(),
            record_every: 1,
            event_tolerance: 1.0e-6,
            halt_at_event: false,
            previous_rate: vec![0.0; dimension],
            guards_before: Vec::new(),
            guards_after: Vec::new(),
        newton_cache: None,
        }
    }

    pub fn energy(&self) -> Option<f64> {
        self.system.energy(self.time, &self.state)
    }

    /// Rate of the last accepted step (zero before the first).
    /// Step-size control. The local error is estimated from how far the
    /// implicit step landed from the explicit prediction `x + h·ẋ_prev`
    /// (differential unknowns only), scaled by `tolerance·(1 + |x|)`. A
    /// step whose estimate exceeds one is rejected and retried shorter;
    /// a quiet one lets the next step grow, up to `h_max`. Events are
    /// still located inside each accepted step. Returns the number of
    /// accepted steps.
    pub fn run_adaptive(&mut self, duration: f64, h0: f64, tolerance: f64, h_min: f64, h_max: f64) -> Result<usize, DynamicsError> {
        let end = self.time + duration;
        let mut h = h0.clamp(h_min, h_max);
        let mut accepted = 0;
        while end - self.time > 1.0e-12 * duration.abs().max(1.0) {
            let h_try = h.min(end - self.time);
            let (time, state, previous_rate) = (self.time, self.state.clone(), self.previous_rate.clone());
            let (events, event_count) = (self.events.len(), self.stats.events);
            let cache = self.newton_cache.take();
            match self.step(h_try) {
                Ok(()) => {
                    let error = self
                        .state
                        .iter()
                        .zip(&state)
                        .zip(&previous_rate)
                        .zip(&self.algebraic)
                        .filter(|(_, algebraic)| !**algebraic)
                        .map(|(((new, old), rate), _)| (new - (old + h_try * rate)).abs() / (tolerance * (1.0 + new.abs())))
                        .fold(0.0_f64, f64::max);
                    let fired = self.events.len() > events;
                    if error > 1.0 && h_try > h_min && !fired {
                        self.time = time;
                        self.state = state;
                        self.previous_rate = previous_rate;
                        self.events.truncate(events);
                        self.stats.events = event_count;
                        self.newton_cache = None;
                        h = (h_try * (0.9 / error.sqrt()).max(0.2)).max(h_min);
                        continue;
                    }
                    accepted += 1;
                    if !fired && error < 0.25 {
                        h = (h_try * (0.9 / error.max(1.0e-6).sqrt()).min(2.0)).min(h_max);
                    } else if fired {
                        h = h_try;
                    }
                }
                Err(e) => {
                    if h_try <= h_min {
                        return Err(e);
                    }
                    self.time = time;
                    self.state = state;
                    self.previous_rate = previous_rate;
                    self.events.truncate(events);
                    self.stats.events = event_count;
                    self.newton_cache = None;
                    h = (h_try * 0.5).max(h_min);
                }
            }
            let _ = cache;
        }
        Ok(accepted)
    }

    pub fn last_rate(&self) -> &[f64] {
        &self.previous_rate
    }

    /// Bring the authored state to consistency with the algebraic
    /// equations. Unknowns are the differential states' rates and, only if
    /// the rates alone cannot satisfy every row, the algebraic values —
    /// an authored pressure or a pinned node keeps its value unless an
    /// equation forces otherwise (a contact's normal force must still be
    /// solved for). Each stage is a minimum-norm Newton (dense
    /// finite-difference Jacobian, SVD pseudo-inverse): rows that hold only
    /// differential values (a pinned angle, a position constraint) have no
    /// unknown here and are skipped when already satisfied, and
    /// underdetermined directions get zero change — the differentiated
    /// constraint, θ̇ = 0 for θ = 0.
    /// The committed state and clock, enough to resume from later with
    /// [`Self::restore`]: the trace, events and statistics are not part of it.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { time: self.time, state: self.state.clone(), previous_rate: self.previous_rate.clone() }
    }

    /// Resume from a snapshot taken on this system; the cached factorisation
    /// is dropped, so the first step after a restore builds a fresh one.
    pub fn restore(&mut self, snapshot: &Snapshot) -> Result<(), DynamicsError> {
        if snapshot.state.len() != self.state.len() {
            return Err(DynamicsError::Dimension { expected: self.state.len(), actual: snapshot.state.len() });
        }
        self.time = snapshot.time;
        self.state.copy_from_slice(&snapshot.state);
        self.previous_rate.copy_from_slice(&snapshot.previous_rate);
        self.newton_cache = None;
        Ok(())
    }

    pub fn make_consistent(&mut self, config: NewtonConfig) -> Result<(), DynamicsError> {
        let Some(algebraic) = self.system.algebraic() else { return Ok(()) };
        let n = self.state.len();
        let algebraic_columns: Vec<usize> = (0..n).filter(|i| algebraic[*i]).collect();
        let differential: Vec<usize> = (0..n).filter(|i| !algebraic[*i]).collect();
        if algebraic_columns.is_empty() {
            return Ok(());
        }
        // Stage 1: rates only. Stage 2: rates and the reaction-like
        // unknowns — algebraic states that appear in exactly one equation
        // (a ground's current, a reservoir's flow), whose authored value is
        // a placeholder. Stage 3: everything, when an authored value must
        // give (a contact's normal force couples into several rows).
        let reactions = self.single_row_algebraic(&algebraic_columns);
        let mut result: Result<(Vec<usize>, Vec<f64>), String> = Err("no stage ran".into());
        let mut stage = "rates only";
        for (label, columns) in [("rates only", Vec::new()), ("rates and reactions", reactions), ("rates and algebraic values", algebraic_columns.clone())] {
            if label != "rates only" && columns.is_empty() {
                continue;
            }
            stage = label;
            result = self.consistent_solve(config, &columns, &differential).map(|(u, worst)| (columns.clone(), u, worst)).and_then(|(c, u, worst)| if worst <= 1.0e-6 || label == "rates and algebraic values" { Ok((c, u)) } else { Err("unreachable rows".into()) });
            if result.is_ok() {
                break;
            }
        }
        if trace_enabled() {
            eprintln!("make_consistent: {stage}: {:?}", result.as_ref().map(|_| ()).map_err(|e| e.to_string()));
        }
        // An inconsistent authored state (a pinned node given a different
        // initial value) simply keeps what was authored.
        if let Ok((columns, unknowns)) = result {
            for (k, c) in columns.iter().enumerate() {
                self.state[*c] = unknowns[k];
            }
            if let Some(last) = self.trace.state.last_mut().filter(|_| self.record_every > 0) {
                last.copy_from_slice(&self.state);
            }
        }
        Ok(())
    }

    /// Algebraic unknowns whose column of the Jacobian (at the authored
    /// state, rates zero) is nonzero in exactly one row: reactions.
    fn single_row_algebraic(&self, algebraic: &[usize]) -> Vec<usize> {
        let n = self.state.len();
        let rate = vec![0.0; n];
        let mut base = vec![0.0; n];
        let mut probe = vec![0.0; n];
        self.system.residual(self.time, &self.state, &rate, &mut base);
        let mut x = self.state.clone();
        algebraic
            .iter()
            .copied()
            .filter(|c| {
                let eps = 1.0e-7 * (1.0 + x[*c].abs());
                x[*c] += eps;
                self.system.residual(self.time, &x, &rate, &mut probe);
                x[*c] -= eps;
                let touched = (0..n).filter(|row| (probe[*row] - base[*row]).abs() > 1.0e-12 * (1.0 + base[*row].abs())).count();
                touched == 1
            })
            .collect()
    }

    /// One minimum-norm Newton solve over `columns` (algebraic values) and
    /// the rates of `differential`. Returns the unknowns and the largest
    /// scaled residual left on rows the unknowns cannot reach.
    fn consistent_solve(&self, config: NewtonConfig, columns: &[usize], differential: &[usize]) -> Result<(Vec<f64>, f64), String> {
        let n = self.state.len();
        let t = self.time;
        let full = self.state.clone();
        let system = &self.system;
        let m = columns.len() + differential.len();
        let mut unknowns = vec![0.0; m];
        for (k, c) in columns.iter().enumerate() {
            unknowns[k] = full[*c];
        }
        let assemble = |u: &[f64], x: &mut [f64], rate: &mut [f64]| {
            x.copy_from_slice(&full);
            rate.iter_mut().for_each(|r| *r = 0.0);
            for (k, c) in columns.iter().enumerate() {
                x[*c] = u[k];
            }
            for (k, d) in differential.iter().enumerate() {
                rate[*d] = u[columns.len() + k];
            }
        };
        let residual = |u: &[f64], r: &mut [f64]| {
            let mut x = vec![0.0; n];
            let mut rate = vec![0.0; n];
            assemble(u, &mut x, &mut rate);
            system.residual(t, &x, &rate, r);
        };
        let mut r = vec![0.0; n];
        let mut probe = vec![0.0; n];
        let mut jacobian = DMatrix::zeros(n, m);
        for iteration in 0..config.max_iterations {
            residual(&unknowns, &mut r);
            if r.iter().any(|v| !v.is_finite()) {
                return Err("residual is not finite".into());
            }
            let norm = r.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
            for c in 0..m {
                let eps = 1.0e-7 * (1.0 + unknowns[c].abs());
                let saved = unknowns[c];
                unknowns[c] += eps;
                residual(&unknowns, &mut probe);
                unknowns[c] = saved;
                for row in 0..n {
                    jacobian[(row, c)] = (probe[row] - r[row]) / eps;
                }
            }
            // Rows no unknown reaches: report how far off they are, scaled
            // by the residual's own size, and leave them to the caller.
            let unreachable: Vec<(usize, f64)> = (0..n)
                .filter(|row| (0..m).all(|c| jacobian[(*row, c)] == 0.0))
                .map(|row| (row, r[row].abs() / (1.0 + norm)))
                .collect();
            let unreachable_left = unreachable.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);
            // Purely algebraic islands have no rates to solve in stage 1.
            // Report their residual so the caller can advance to solving
            // algebraic values; an empty Jacobian has no SVD to compute.
            if m == 0 {
                return Ok((unknowns, unreachable_left));
            }
            if trace_enabled() && unreachable_left > 1.0e-6 {
                let worst = unreachable.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
                eprintln!("consistent_solve ({} columns): row {} unreachable, residual {:.3e}", m, worst.0, r[worst.0]);
            }
            let rhs = DVector::from_iterator(n, r.iter().map(|v| -v));
            let svd = jacobian.clone().svd(true, true);
            let largest = svd.singular_values.iter().cloned().fold(0.0_f64, f64::max);
            let Ok(delta) = svd.solve(&rhs, 1.0e-10 * largest.max(1.0e-300)) else {
                return Err("pseudo-inverse failed".into());
            };
            let step = delta.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
            for c in 0..m {
                unknowns[c] += delta[c];
            }
            let scale = unknowns.iter().fold(1.0_f64, |m, v| m.max(v.abs()));
            if step <= config.relative_tolerance * scale || norm <= config.absolute_tolerance {
                let _ = iteration;
                return Ok((unknowns, unreachable_left));
            }
        }
        Err("did not converge".into())
    }

    pub fn run(&mut self, duration: f64, h: f64) -> Result<(), DynamicsError> {
        let end = self.time + duration;
        let count = (duration / h).round().max(1.0) as u64;
        for index in 0..count {
            let remaining = end - self.time;
            // A last step within rounding of `h` is taken as exactly `h`
            // so the cached factorisation keyed on the step still matches.
            let step = if index + 1 == count { remaining } else { h.min(remaining) };
            if step <= 0.0 {
                break;
            }
            self.step(step)?;
        }
        self.time = end;
        Ok(())
    }

    /// Advance until the next event fires, stopping exactly at it with the
    /// jump applied, or until `max_duration` elapses. This is the Poincaré
    /// map of a hybrid system: strike to strike, bounce to bounce.
    pub fn run_to_event(&mut self, max_duration: f64, h: f64) -> Result<Option<Event>, DynamicsError> {
        let end = self.time + max_duration;
        let count = self.events.len();
        self.halt_at_event = true;
        let result = (|| {
            while self.time < end && self.events.len() == count {
                let step = h.min(end - self.time);
                if step <= 0.0 {
                    break;
                }
                self.step(step)?;
            }
            Ok(())
        })();
        self.halt_at_event = false;
        result?;
        Ok(self.events.get(count).copied())
    }

    /// Advance exactly one step of `h`, resolving any events inside it.
    pub fn step(&mut self, h: f64) -> Result<(), DynamicsError> {
        profile::STEP.time(|| self.step_inner(h))
    }

    fn step_inner(&mut self, h: f64) -> Result<(), DynamicsError> {
        if h <= 0.0 || !h.is_finite() {
            return Err(DynamicsError::InvalidStep(h));
        }
        if self.state.len() != self.system.dimension() {
            return Err(DynamicsError::Dimension {
                expected: self.system.dimension(),
                actual: self.state.len(),
            });
        }
        let mut remaining = h;
        let mut depth = 0;
        while remaining > 0.0 {
            self.guards_before.clear();
            profile::GUARDS.time(|| self.system.guards(self.time, &self.state, &mut self.guards_before));
            let mut candidate = self.state.clone();
            self.advance(self.time, remaining, &mut candidate)?;
            self.guards_after.clear();
            profile::GUARDS.time(|| self.system.guards(self.time + remaining, &candidate, &mut self.guards_after));
            let crossing = self
                .guards_before
                .iter()
                .zip(&self.guards_after)
                .position(|(before, after)| *before >= 0.0 && *after < 0.0);
            match crossing {
                None => {
                    self.commit(self.time + remaining, candidate)?;
                    remaining = 0.0;
                }
                Some(guard) => {
                    if trace_enabled() {
                        eprintln!("event guard {guard} at t={} h={remaining}: before {:?} after {:?} state {:?} candidate {:?}", self.time, self.guards_before, self.guards_after, &self.state[..self.state.len().min(9)], &candidate[..candidate.len().min(9)]);
                    }
                    // The trial steps of the search would each evict the
                    // step's factorisation for their own; keep it aside.
                    let kept = self.newton_cache.take();
                    let dt = self.locate_event(guard, remaining);
                    self.newton_cache = kept;
                    let mut dt = dt?;
                    if dt > 0.0 {
                        let mut at_event = self.state.clone();
                        match self.advance(self.time, dt, &mut at_event) {
                            Ok(()) => self.commit(self.time + dt, at_event)?,
                            // A tolerance-sized step that will not converge
                            // (a rigid contact at a sample instant is too
                            // stiff for it): the event fires at the state
                            // already committed, a tolerance early.
                            Err(DynamicsError::Solve { .. }) if dt <= 2.0 * self.event_tolerance * remaining => dt = 0.0,
                            Err(e) => return Err(e),
                        }
                    }
                    let time = self.time;
                    profile::JUMP.time(|| self.system.jump(guard, time, &mut self.state));
                    self.previous_rate.iter_mut().for_each(|r| *r = 0.0);
                    self.newton_cache = None;
                    self.events.push(Event { time, guard });
                    self.stats.events += 1;
                    // Simultaneous crossings: the located time sits a
                    // tolerance past the instant, so another guard that was
                    // non-negative at the start of the step and is negative
                    // now crossed inside it. It fires here too — two
                    // samplers due at the same tick both tick.
                    let mut fired = vec![guard];
                    loop {
                        let mut now = Vec::new();
                        self.system.guards(time, &self.state, &mut now);
                        let next = self.guards_before.iter().zip(&now).enumerate().position(|(k, (before, after))| *before >= 0.0 && *after < 0.0 && !fired.contains(&k));
                        let Some(k) = next else { break };
                        if trace_enabled() {
                            eprintln!("simultaneous event guard {k} at t={time}");
                        }
                        profile::JUMP.time(|| self.system.jump(k, time, &mut self.state));
                        self.events.push(Event { time, guard: k });
                        self.stats.events += 1;
                        fired.push(k);
                    }
                    if self.halt_at_event {
                        return Ok(());
                    }
                    remaining -= dt;
                    depth += 1;
                    if depth > 64 {
                        // Zeno accumulation: finish the step without further events.
                        let mut candidate = self.state.clone();
                        self.advance(self.time, remaining, &mut candidate)?;
                        self.commit(self.time + remaining, candidate)?;
                        remaining = 0.0;
                    }
                }
            }
        }
        Ok(())
    }

    fn commit(&mut self, time: f64, state: Vec<f64>) -> Result<(), DynamicsError> {
        if state.iter().any(|value| !value.is_finite()) {
            return Err(DynamicsError::NonFinite(time));
        }
        let dt = time - self.time;
        if dt > 0.0 {
            for (rate, (new, old)) in self.previous_rate.iter_mut().zip(state.iter().zip(&self.state)) {
                *rate = (new - old) / dt;
            }
        }
        self.state = state;
        self.time = time;
        self.stats.steps += 1;
        if self.record_every > 0 && self.stats.steps.is_multiple_of(self.record_every) {
            let energy = self.system.energy(self.time, &self.state);
            self.trace.push(self.time, &self.state, energy);
        }
        Ok(())
    }

    /// Bisect for the smallest sub-step on which `guard` has crossed, so the
    /// committed state sits just past the crossing: a jump that leaves the
    /// guard untouched (an escapement kick, a leg swap) then cannot re-fire
    /// on the same crossing.
    fn locate_event(&mut self, guard: usize, h: f64) -> Result<f64, DynamicsError> {
        profile::LOCATE.time(|| self.locate_event_inner(guard, h))
    }

    fn locate_event_inner(&mut self, guard: usize, h: f64) -> Result<f64, DynamicsError> {
        let tolerance = self.event_tolerance;
        // The shortest event step: the power-of-two fraction of the step
        // just under the tolerance, as a bisection to that tolerance ends on.
        let epsilon = h * 0.5_f64.powi((1.0 / tolerance).log2().ceil() as i32);
        let (mut low, mut high) = (0.0, h);
        let (mut f_low, mut f_high) = (self.guards_before[guard], self.guards_after[guard]);
        // The step that ends at the event is kept short — a tolerance of
        // the step when the guard is already on its crossing — because a
        // committed rate lane holds the step's average rate, and a jump
        // that samples a rate (a tachometer, a step-average sensor) must
        // see the instant, not the mean of a whole step.
        if f_low == 0.0 {
            return Ok(epsilon);
        }
        // A guard is located either in time, to `tolerance` of the step,
        // or in value, when it has shrunk to `tolerance` of its swing over
        // the step: a clock guard is linear in time and the first secant
        // lands on it exactly, where bisection would take twenty trials.
        let band = tolerance * f_low.abs().max(f_high.abs());
        let mut scratch = vec![0.0; self.state.len()];
        let mut values = Vec::new();
        // Regula falsi with the Illinois correction, falling back to the
        // midpoint whenever the secant hugs an end of the bracket.
        let mut side = 0i8;
        let mut slow = 0u8;

        while high - low > tolerance * h {
            let width = high - low;
            let secant = if f_low > 0.0 && f_high < 0.0 { low + width * f_low / (f_low - f_high) } else { low + 0.5 * width };
            // The secant, held a tolerance inside the bracket so a crossing
            // at the step's very start or end is settled by one trial; the
            // midpoint whenever the secant has stopped shrinking the bracket.
            let (inner_low, inner_high) = (low + epsilon, high - epsilon);
            let mid = if slow >= 2 || inner_low >= inner_high { low + 0.5 * width } else { secant.clamp(inner_low, inner_high) };
            scratch.copy_from_slice(&self.state);
            self.advance(self.time, mid, &mut scratch)?;
            values.clear();
            self.system.guards(self.time + mid, &scratch, &mut values);
            let f = values[guard];
            if f.abs() <= band || (f < 0.0 && mid - low <= tolerance * h) {
                // On the crossing to within the band: commit just past it.
                return Ok(if f < 0.0 { mid } else { (mid + epsilon).min(h) });
            }
            if f >= 0.0 && high - mid <= tolerance * h {
                return Ok(high);
            }
            slow = if (if f >= 0.0 { high - mid } else { mid - low }) > 0.5 * width { slow + 1 } else { 0 };
            if f >= 0.0 {
                low = mid;
                f_low = f;
                if side == 1 {
                    f_high *= 0.5;
                }
                side = 1;
            } else {
                high = mid;
                f_high = f;
                if side == -1 {
                    f_low *= 0.5;
                }
                side = -1;
            }
        }
        Ok(high)
    }

    fn advance(&mut self, t: f64, h: f64, x: &mut [f64]) -> Result<(), DynamicsError> {
        self.advance_subdividing(t, h, x, 0)
    }

    /// One step; an implicit solve that fails to converge (an impact, a
    /// mode switch inside the step) is retried as two half steps, up to
    /// `MAX_SUBDIVISION` levels deep, before the error is reported.
    fn advance_subdividing(&mut self, t: f64, h: f64, x: &mut [f64], depth: u32) -> Result<(), DynamicsError> {
        const MAX_SUBDIVISION: u32 = 6;
        match self.integrator {
            Integrator::Rk4 => rk4(&self.system, t, h, x),
            Integrator::ImplicitMidpoint(config) | Integrator::BackwardEuler(config) => {
                let attempt = x.to_vec();
                // Retries after a failed solve use backward Euler: first order
                // but L-stable, which is what a stiff constitutive kink needs.
                let theta = if depth == 0 && matches!(self.integrator, Integrator::ImplicitMidpoint(_)) { 0.5 } else { 1.0 };
                self.system.begin_step(h);
                // The cached factorisation is only meaningful for the same
                // rule and (to a part in ten thousand, which Newton's
                // contraction test absorbs) the same step; otherwise fresh.
                let mut cache = match self.newton_cache.take() {
                    Some((ch, ctheta, c)) if (ch - h).abs() <= 1.0e-4 * h && ctheta == theta => Some(c),
                    _ => None,
                };
                let mut result = profile::IMPLICIT.time(|| implicit_step(&self.system, t, h, x, config, &self.previous_rate, theta, &self.sparsity, &self.algebraic, None, &mut cache));
                if trace_enabled() {
                    if let Err(e) = &result { eprintln!("smooth attempt failed: {e}"); }
                }
                if matches!(result, Err(DynamicsError::Solve { source: SolveError::NotConverged { .. } | SolveError::Singular { .. }, .. })) {
                    // The smooth predictor found no solution: try the
                    // branches the system's nonsmooth elements propose.
                    // A branch is an impulse, so it takes the backward
                    // Euler step: the midpoint rule would satisfy the
                    // constraint halfway and reflect the velocity.
                    for branch in self.system.branches(t, &attempt) {
                        if trace_enabled() {
                            eprintln!("branch restart at t={t} h={h}: {:?}", &branch[..branch.len().min(9)]);
                        }
                        x.copy_from_slice(&attempt);
                        // A branch is a different mode: no reuse across it.
                        cache = None;
                        result = profile::IMPLICIT.time(|| implicit_step(&self.system, t, h, x, config, &self.previous_rate, 1.0, &self.sparsity, &self.algebraic, Some(&branch), &mut None));
                        if result.is_ok() {
                            self.stats.branch_restarts += 1;
                            break;
                        }
                    }
                }
                match result {
                    Ok(iterations) => {
                        self.stats.max_newton_iterations = self.stats.max_newton_iterations.max(iterations);
                        if let Some(c) = cache {
                            self.newton_cache = Some((h, theta, c));
                        }
                        Ok(())
                    }
                    Err(DynamicsError::Solve { source: SolveError::NotConverged { .. } | SolveError::Singular { .. }, .. })
                        if depth < MAX_SUBDIVISION =>
                    {
                        x.copy_from_slice(&attempt);
                        self.stats.subdivided_steps += 1;
                        self.advance_subdividing(t, 0.5 * h, x, depth + 1)?;
                        self.advance_subdividing(t + 0.5 * h, 0.5 * h, x, depth + 1)
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }
}

fn rk4<S: System>(system: &S, t: f64, h: f64, x: &mut [f64]) -> Result<(), DynamicsError> {
    let n = x.len();
    let (mut k1, mut k2, mut k3, mut k4, mut y) =
        (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    if !system.derivative(t, x, &mut k1) {
        return Err(DynamicsError::NotExplicit);
    }
    for i in 0..n {
        y[i] = x[i] + 0.5 * h * k1[i];
    }
    system.derivative(t + 0.5 * h, &y, &mut k2);
    for i in 0..n {
        y[i] = x[i] + 0.5 * h * k2[i];
    }
    system.derivative(t + 0.5 * h, &y, &mut k3);
    for i in 0..n {
        y[i] = x[i] + h * k3[i];
    }
    system.derivative(t + h, &y, &mut k4);
    for i in 0..n {
        x[i] += h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
    }
    Ok(())
}

/// One-stage implicit step: `theta = 0.5` is the implicit midpoint rule,
/// `theta = 1` backward Euler. Algebraic unknowns are always taken at the
/// end of the step.
///
/// Newton's unknown is the *increment* `u = x_new − x_old`, not the new
/// value: a 300 K node moving by 10 µK per step then has an unknown of
/// size 10 µK, so finite-difference steps, scaling and the stopping test
/// all follow the step's own magnitude instead of the state's.
#[allow(clippy::too_many_arguments)]
fn implicit_step<S: System>(
    system: &S,
    t: f64,
    h: f64,
    x: &mut [f64],
    config: NewtonConfig,
    previous_rate: &[f64],
    theta: f64,
    sparsity: &Sparsity,
    algebraic: &[bool],
    start: Option<&[f64]>,
    cache: &mut Option<JacobianCache>,
) -> Result<usize, DynamicsError> {
    let n = x.len();
    let old = x.to_vec();
    if trace_enabled() {
        eprintln!("implicit_step t={t} h={h} theta={theta} algebraic={algebraic:?} old={:?}", &old[..n.min(9)]);
    }
    // Predictor: the explicit derivative when the system has one, otherwise
    // the last accepted step's rate.
    let mut u = vec![0.0; n];
    let mut rate = vec![0.0; n];
    if let Some(start) = start {
        for i in 0..n {
            u[i] = start[i] - old[i];
        }
    } else if system.derivative(t, &old, &mut rate) {
        for i in 0..n {
            u[i] = h * rate[i];
        }
    } else {
        for i in 0..n {
            u[i] = h * previous_rate[i];
        }
    }
    let stage = |u: &[f64], mid: &mut [f64], rate: &mut [f64]| {
        for i in 0..n {
            mid[i] = if algebraic[i] { old[i] + u[i] } else { old[i] + theta * u[i] };
            rate[i] = u[i] / h;
        }
    };
    // One pair of stage buffers for the whole solve, not one per evaluation.
    let stage_buffers = std::cell::RefCell::new((vec![0.0; n], vec![0.0; n]));
    let residual = |u: &[f64], residual: &mut [f64]| {
        let mut buffers = stage_buffers.borrow_mut();
        let (mid, rate) = &mut *buffers;
        stage(u, mid, rate);
        system.residual(t + theta * h, mid, rate, residual);
    };
    let mut parts = JacobianParts::default();
    let mut scratch = vec![0.0; n];
    // Perturbations on the increment's own scale, never below what the
    // absolute value can resolve.
    let epsilon = |i: usize, value: f64| {
        let scale = if algebraic[i] { 1.0 + (old[i] + value).abs() } else { h + value.abs() };
        (1.0e-6 * scale).max(1.0e-13 * (1.0 + old[i].abs()))
    };
    // Stop when the correction is negligible on the increment's scale, or
    // below what the absolute value can resolve (1e-4 of the value times
    // the solver's 1e-8 relative tolerance is 1e-12 relative).
    let step_scale = |i: usize, value: f64| {
        let absolute = 1.0e-4 * (1.0 + (old[i] + value).abs());
        if algebraic[i] { (1.0 + (old[i] + value).abs()).max(absolute) } else { (h + value.abs()).max(absolute) }
    };
    let diagnostics = solve_newton_cached(&mut u, config, residual, |next, base, jacobian| {
        let mut mid = vec![0.0; n];
        let mut rate = vec![0.0; n];
        stage(next, &mut mid, &mut rate);
        parts.clear();
        if system.jacobian(t + theta * h, &mid, &rate, &mut parts) {
            // `d(mid)/du` is θ for differential unknowns and 1 for algebraic
            // ones; `d(rate)/du` is 1/h.
            for (r, c, v) in &parts.d_dx {
                let weight = if algebraic[*c] { 1.0 } else { theta };
                jacobian.add(*r, *c, weight * v);
            }
            for (r, c, v) in &parts.d_drate {
                jacobian.add(*r, *c, v / h);
            }
        } else {
            scratch.copy_from_slice(next);
            sparsity.finite_difference_sparse(&mut scratch, base, jacobian, &epsilon, |uu, out| {
                let mut mid = vec![0.0; n];
                let mut rate = vec![0.0; n];
                stage(uu, &mut mid, &mut rate);
                system.residual(t + theta * h, &mid, &rate, out);
            });
        }
    }, &step_scale, cache)
    .map_err(|source| DynamicsError::Solve { time: t, source })?;
    for i in 0..n {
        x[i] = old[i] + u[i];
    }
    Ok(diagnostics.iterations)
}

#[cfg(test)]
mod adaptive_tests {
    use super::*;

    struct Decay;
    impl Ode for Decay {
        fn dimension(&self) -> usize { 1 }
        fn derivative(&self, _t: f64, x: &[f64], dxdt: &mut [f64]) { dxdt[0] = -50.0 * x[0]; }
    }

    #[test]
    fn adaptive_steps_grow_once_the_transient_is_over() {
        let mut sim = Simulation::new(Decay, Integrator::implicit_midpoint(), vec![1.0]);
        sim.record_every = 0;
        let steps = sim.run_adaptive(2.0, 1.0e-3, 1.0e-4, 1.0e-5, 0.5).unwrap();
        assert!((sim.state[0] - (-100.0_f64).exp()).abs() < 1.0e-3, "{}", sim.state[0]);
        assert!(steps < 400, "took {steps} steps where a fixed 1 ms grid takes 2000");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlgebraicConstraints;
    impl System for AlgebraicConstraints {
        fn dimension(&self) -> usize { 2 }
        fn algebraic(&self) -> Option<Vec<bool>> { Some(vec![true, true]) }
        fn residual(&self, _t: f64, x: &[f64], _rate: &[f64], r: &mut [f64]) {
            r[0] = x[0] + x[1] - 5.0;
            r[1] = x[0] - x[1] - 1.0;
        }
    }

    #[test]
    fn purely_algebraic_initialization_solves_constraints_and_records_the_solution() {
        for initial in [vec![0.0, 0.0], vec![3.0, 2.0]] {
            let mut sim = Simulation::new(AlgebraicConstraints, Integrator::implicit_midpoint(), initial);
            sim.make_consistent(NewtonConfig::default()).unwrap();
            assert!((sim.state[0] - 3.0).abs() < 1.0e-9);
            assert!((sim.state[1] - 2.0).abs() < 1.0e-9);
            assert_eq!(sim.trace.state[0], sim.state);
            sim.run(0.1, 0.01).unwrap();
            assert!((sim.state[0] - 3.0).abs() < 1.0e-9);
            assert!((sim.state[1] - 2.0).abs() < 1.0e-9);
        }
    }

    struct Harmonic;
    impl Ode for Harmonic {
        fn dimension(&self) -> usize {
            2
        }
        fn derivative(&self, _t: f64, x: &[f64], dxdt: &mut [f64]) {
            dxdt[0] = x[1];
            dxdt[1] = -x[0];
        }
        fn energy(&self, _t: f64, x: &[f64]) -> Option<f64> {
            Some(0.5 * (x[0] * x[0] + x[1] * x[1]))
        }
    }

    fn endpoint(integrator: Integrator, h: f64) -> f64 {
        let mut sim = Simulation::new(Harmonic, integrator, vec![1.0, 0.0]);
        sim.record_every = 0;
        sim.run(2.0, h).unwrap();
        sim.state[0]
    }

    #[test]
    fn midpoint_is_second_order_and_rk4_is_fourth_order() {
        let exact = 2.0_f64.cos();
        let e1 = (endpoint(Integrator::implicit_midpoint(), 0.02) - exact).abs();
        let e2 = (endpoint(Integrator::implicit_midpoint(), 0.01) - exact).abs();
        assert!((e1 / e2 - 4.0).abs() < 0.3, "midpoint ratio {}", e1 / e2);
        let e1 = (endpoint(Integrator::Rk4, 0.02) - exact).abs();
        let e2 = (endpoint(Integrator::Rk4, 0.01) - exact).abs();
        assert!((e1 / e2 - 16.0).abs() < 1.5, "rk4 ratio {}", e1 / e2);
    }

    #[test]
    fn midpoint_conserves_quadratic_energy_exactly() {
        let mut sim = Simulation::new(Harmonic, Integrator::implicit_midpoint(), vec![1.0, 0.0]);
        sim.run(50.0, 0.05).unwrap();
        assert!((sim.energy().unwrap() - 0.5).abs() < 1.0e-9);
    }

    /// A ball that bounces: guard is height, jump reverses velocity.
    struct Ball;
    impl Ode for Ball {
        fn dimension(&self) -> usize {
            2
        }
        fn derivative(&self, _t: f64, x: &[f64], dxdt: &mut [f64]) {
            dxdt[0] = x[1];
            dxdt[1] = -1.0;
        }
        fn guards(&self, _t: f64, x: &[f64], guards: &mut Vec<f64>) {
            guards.push(x[0]);
        }
        fn jump(&mut self, _index: usize, _t: f64, x: &mut [f64]) {
            x[1] = -x[1];
        }
    }

    #[test]
    fn run_to_event_stops_exactly_at_the_jump() {
        let mut sim = Simulation::new(Ball, Integrator::Rk4, vec![1.0, 0.0]);
        let event = sim.run_to_event(10.0, 0.01).unwrap().unwrap();
        assert!((sim.time - event.time).abs() < 1.0e-12);
        assert!(sim.state[0].abs() < 1.0e-6 && sim.state[1] > 0.0);
        assert!(sim.run_to_event(0.05, 0.01).unwrap().is_none());
    }

    /// A guard the jump does not move: the event must fire exactly once.
    struct Kicked;
    impl Ode for Kicked {
        fn dimension(&self) -> usize {
            2
        }
        fn derivative(&self, _t: f64, x: &[f64], dxdt: &mut [f64]) {
            dxdt[0] = x[1];
            dxdt[1] = -x[0];
        }
        fn guards(&self, _t: f64, x: &[f64], guards: &mut Vec<f64>) {
            guards.push(x[0]);
        }
        fn jump(&mut self, _index: usize, _t: f64, x: &mut [f64]) {
            x[1] *= 1.01;
        }
    }

    #[test]
    fn a_jump_that_keeps_the_guard_fires_once_per_crossing() {
        let mut sim = Simulation::new(Kicked, Integrator::Rk4, vec![1.0, 0.0]);
        sim.run(2.0 * std::f64::consts::PI * 3.0, 0.01).unwrap();
        assert_eq!(sim.events.len(), 3);
    }

    #[test]
    fn events_are_located_to_tolerance() {
        let mut sim = Simulation::new(Ball, Integrator::Rk4, vec![1.0, 0.0]);
        sim.run(2.0, 0.01).unwrap();
        assert_eq!(sim.events.len(), 1);
        assert!((sim.events[0].time - 2.0_f64.sqrt()).abs() < 1.0e-7);
        assert!(sim.state[0] > 0.0);
    }

    /// Index-1 DAE: x' = -y, y = x (algebraic row) so x decays as e^{-t}.
    struct Dae;
    impl System for Dae {
        fn dimension(&self) -> usize {
            2
        }
        fn residual(&self, _t: f64, x: &[f64], rate: &[f64], r: &mut [f64]) {
            r[0] = rate[0] + x[1];
            r[1] = x[1] - x[0];
        }
    }

    #[test]
    fn implicit_midpoint_solves_algebraic_rows() {
        let mut sim = Simulation::new(Dae, Integrator::implicit_midpoint(), vec![1.0, 1.0]);
        sim.run(1.0, 1.0e-3).unwrap();
        assert!((sim.state[0] - (-1.0_f64).exp()).abs() < 1.0e-6);
        assert!(matches!(
            Simulation::new(Dae, Integrator::Rk4, vec![1.0, 1.0]).step(0.1),
            Err(DynamicsError::NotExplicit)
        ));
    }
}

/// `SIM_NEWTON_TRACE=1` prints every Newton iteration, step header, branch
/// restart and event crossing to stderr — the first thing to reach for
/// when a step will not converge.
fn trace_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("SIM_NEWTON_TRACE").is_some())
}

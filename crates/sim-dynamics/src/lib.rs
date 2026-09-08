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

pub mod event_root;
pub mod hybrid;
pub mod analysis;
pub mod jacobian;
pub mod linear;
pub mod report;
pub mod jacobian_check;
pub mod attempt_check;
pub mod sdirk;

use jacobian::Sparsity;
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};
use sim_solve::{JacobianCache, NewtonConfig, SolveError, profile, solve_newton_cached_audited};
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
    /// which a guard goes from nonnegative to negative; the step is then
    /// bisected to locate the crossing before [`System::jump`] is applied.
    fn guards(&self, _t: f64, _x: &[f64], _guards: &mut Vec<f64>) {}

    /// Known clock deadlines `(guard index, absolute time)`, constant during
    /// continuous advancement until a jump updates/removes them. Endpoint
    /// ticks fire before a step returns; these guards bypass root finding.
    fn scheduled_events(&self, _t: f64, _x: &[f64], _events: &mut Vec<(usize, f64)>) {}

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
    fn scheduled_events(&self, _t: f64, _x: &[f64], _events: &mut Vec<(usize, f64)>) {}
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
    fn scheduled_events(&self, t: f64, x: &[f64], events: &mut Vec<(usize, f64)>) {
        Ode::scheduled_events(self, t, x, events)
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
    #[error("integration breakpoints must be finite, nonnegative and strictly increasing")]
    InvalidBreakpoints,
    #[error("invalid scheduled event at t={time}: {reason}")]
    Schedule { time: f64, reason: &'static str },
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

/// An attempted nonlinear solve, including rejected/root-search trials. A
/// successful solve is not proof its enclosing timestep was finally committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplicitAttempt {
    pub start_time: f64,
    pub step: f64,
    pub theta: f64,
    pub stage_time: f64,
    pub subdivision_depth: u32,
    pub branch: bool,
    pub solve_succeeded: bool,
    /// Whether this solve's substep survived a local Simulation commit.
    /// False includes successful discarded candidates/event-search probes.
    /// None means unknown in legacy captures. This does not certify a later
    /// outer coupled-runtime transaction or physical accuracy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed: Option<bool>,
    pub error: Option<String>,
    pub initial_state: Vec<f64>,
    pub stage_state: Vec<f64>,
    pub stage_rate: Vec<f64>,
    pub residual: Vec<f64>,
    pub newton: sim_solve::NewtonAudit,
    /// Last fresh matrix build in this trial; absent when only a cached matrix
    /// was used, and in legacy records. Captured only with the attempt audit.
    #[serde(default)]
    pub last_linearization: Option<ImplicitLinearization>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImplicitLinearization {
    pub increment: Vec<f64>,
    pub residual: Vec<f64>,
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
    /// Experimental modified Newton across nearby event-location trials.
    /// A reused matrix is treated as stale and may be refreshed by Newton.
    pub event_jacobian_reuse: bool,
    locating_event: bool,
    /// Absolute requested step boundaries; retained across state restoration.
    step_breakpoints: Vec<f64>,
    use_provided_jacobian: bool,
    attempt_limit: usize,
    pub implicit_attempts: Vec<ImplicitAttempt>,
}

impl<S: System> Simulation<S> {
    /// Enable bounded solve-point capture. Zero disables capture; changing the
    /// limit clears existing records. Diagnostic re-evaluations are opt-in.
    pub fn set_attempt_audit_limit(&mut self, limit: usize) {
        self.attempt_limit = limit;
        self.implicit_attempts.clear();
    }

    pub fn attempt_audit_limit(&self) -> usize { self.attempt_limit }

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
        event_jacobian_reuse: false,
        locating_event: false,
        step_breakpoints: Vec::new(),
        use_provided_jacobian: true,
        attempt_limit: 0,
        implicit_attempts: Vec::new(),
        }
    }

    pub fn energy(&self) -> Option<f64> {
        self.system.energy(self.time, &self.state)
    }

    /// Request additional integration boundaries without adding events or
    /// resetting held controller states. Useful for comparing derivative paths
    /// on the same time grid. Boundaries do not disable convergence subdivision.
    /// Past boundaries are ignored; retaining the full list makes snapshot
    /// restoration and rejected adaptive trials replay the same configuration.
    pub fn set_step_breakpoints(&mut self, points: Vec<f64>) -> Result<(), DynamicsError> {
        if points.iter().any(|t| !t.is_finite() || *t < 0.0)
            || points.windows(2).any(|p| p[0] >= p[1]) {
            return Err(DynamicsError::InvalidBreakpoints);
        }
        self.step_breakpoints = points;
        Ok(())
    }

    /// Independently difference the complete stage residual, bypassing both
    /// provided partial derivatives and their sparsity pattern. Intended for
    /// validation/replay comparisons; it is usually slower. Switching clears
    /// cached factorizations so no provided derivative leaks into the reference.
    pub fn set_numerical_jacobian(&mut self, enabled: bool) {
        self.use_provided_jacobian = !enabled;
        let n = self.state.len();
        self.sparsity = if enabled { None } else { self.system.sparsity() }
            .unwrap_or_else(|| Sparsity::new((0..n).map(|_| (0..n).collect()).collect()));
        self.newton_cache = None;
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
        // Retain the attempted work, but superseded future substeps no longer
        // belong to the restored trajectory (including outer runtime retries).
        for attempt in &mut self.implicit_attempts {
            if attempt.committed == Some(true) && attempt.start_time + attempt.step > snapshot.time {
                attempt.committed = Some(false);
            }
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
        let end = self.time + h;
        let mut depth = 0;
        let mut due_jumps = 0;
        loop {
            let mut scheduled = Vec::new();
            self.system.scheduled_events(self.time, &self.state, &mut scheduled);
            if scheduled.iter().any(|(_, deadline)| !deadline.is_finite()) {
                return Err(DynamicsError::Schedule { time: self.time, reason: "non-finite deadline" });
            }
            scheduled.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let roundoff = |deadline: f64| 64.0 * f64::EPSILON
                * self.time.abs().max(deadline.abs()).max(h);
            if let Some(&(guard, deadline)) = scheduled.first().filter(|(_, deadline)| *deadline <= self.time + roundoff(*deadline)) {
                if due_jumps >= 1024 {
                    return Err(DynamicsError::Schedule { time: self.time, reason: "clock did not advance" });
                }
                profile::JUMP.time(|| self.system.jump(guard, self.time, &mut self.state));
                self.previous_rate.fill(0.0);
                self.newton_cache = None;
                self.events.push(Event { time: self.time, guard });
                self.stats.events += 1;
                due_jumps += 1;
                let mut after = Vec::new();
                self.system.scheduled_events(self.time, &self.state, &mut after);
                if after.iter().any(|(i, next)| *i == guard && *next <= deadline) {
                    return Err(DynamicsError::Schedule { time: self.time, reason: "jump must advance or remove its deadline" });
                }
                if self.halt_at_event { return Ok(()); }
                continue;
            }
            let mut remaining = (end - self.time).max(0.0);
            if remaining == 0.0 { break; }
            if let Some(&(_, deadline)) = scheduled.first() {
                // A clock at the requested endpoint within floating-point
                // roundoff fires there. Splitting off its ulp-sized remainder
                // would create an artificial, often singular implicit step.
                if deadline < end - roundoff(end) {
                    remaining = remaining.min(deadline - self.time);
                }
            }
            let next = self.step_breakpoints.partition_point(|t| *t <= self.time + roundoff(*t));
            if let Some(&boundary) = self.step_breakpoints.get(next) {
                if boundary < end - roundoff(end) {
                    remaining = remaining.min(boundary - self.time);
                }
            }
            due_jumps = 0;
            self.guards_before.clear();
            profile::GUARDS.time(|| self.system.guards(self.time, &self.state, &mut self.guards_before));
            let mut candidate = self.state.clone();
            let trial_audit_start = self.implicit_attempts.len();
            self.advance(self.time, remaining, &mut candidate)?;
            self.guards_after.clear();
            profile::GUARDS.time(|| self.system.guards(self.time + remaining, &candidate, &mut self.guards_after));
            let crossings: Vec<_> = self
                .guards_before
                .iter()
                .zip(&self.guards_after)
                .enumerate()
                .filter_map(|(i, (before, after))| (*before >= 0.0 && *after < 0.0 && !scheduled.iter().any(|(guard, _)| *guard == i)).then_some(i)).collect();
            match crossings.first().copied() {
                None => {
                    self.commit_trial(self.time + remaining, candidate, trial_audit_start)?;
                }
                Some(mut guard) => {
                    if trace_enabled() {
                        eprintln!("event guard {guard} at t={} h={remaining}: before {:?} after {:?} state {:?} candidate {:?}", self.time, self.guards_before, self.guards_after, &self.state[..self.state.len().min(9)], &candidate[..candidate.len().min(9)]);
                    }
                    // The trial steps of the search would each evict the
                    // step's factorisation for their own; keep it aside.
                    let kept = self.newton_cache.take();
                    // Declaration order is not event order. Locate all guards
                    // that crossed in this trial, then commit the earliest;
                    // its jump may invalidate later crossings altogether.
                    let dt = (|| {
                        let mut first = self.locate_event(guard, remaining)?;
                        for &other in &crossings[1..] {
                            let candidate = self.locate_event(other, remaining)?;
                            if candidate < first { first=candidate; guard=other; }
                        }
                        Ok::<f64,DynamicsError>(first)
                    })();
                    self.newton_cache = kept;
                    let mut dt = dt?;
                    if dt > 0.0 {
                        let mut at_event = self.state.clone();
                        let event_audit_start = self.implicit_attempts.len();
                        match self.advance(self.time, dt, &mut at_event) {
                            Ok(()) => self.commit_trial(self.time + dt, at_event, event_audit_start)?,
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
                        let next = self.guards_before.iter().zip(&now).enumerate().position(|(k, (before, after))| *before >= 0.0 && *after < 0.0 && !fired.contains(&k) && !scheduled.iter().any(|(guard, _)| *guard == k));
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
                        let trial_audit_start = self.implicit_attempts.len();
                        self.advance(self.time, remaining, &mut candidate)?;
                        self.commit_trial(self.time + remaining, candidate, trial_audit_start)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn commit_trial(&mut self, time: f64, state: Vec<f64>, audit_start: usize) -> Result<(), DynamicsError> {
        self.commit(time, state)?;
        // Only the successful leaves of THIS advance belong to its committed
        // candidate. Earlier full-step/event-search trials stay uncommitted.
        // If recursive subdivision fails after a successful prefix, no commit
        // occurs and that prefix correctly remains uncommitted too.
        for attempt in &mut self.implicit_attempts[audit_start..] {
            if attempt.solve_succeeded { attempt.committed = Some(true); }
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
        let previous = self.locating_event;
        self.locating_event = true;
        let result = profile::LOCATE.time(|| self.locate_event_inner(guard, h));
        self.locating_event = previous;
        result
    }

    fn locate_event_inner(&mut self, guard: usize, h: f64) -> Result<f64, DynamicsError> {
        let bracket = event_root::CrossingBracket {
            duration: h, relative_tolerance: self.event_tolerance,
            before: self.guards_before[guard], after: self.guards_after[guard],
        };
        let mut scratch = vec![0.0; self.state.len()];
        let mut values = Vec::new();
        event_root::locate_crossing(bracket, |dt| {
            scratch.copy_from_slice(&self.state);
            self.advance(self.time, dt, &mut scratch)?;
            values.clear();
            self.system.guards(self.time + dt, &scratch, &mut values);
            values.get(guard).copied().ok_or(DynamicsError::Schedule { time:self.time, reason:"guard layout changed during event location" })
        }).map_err(|e| match e {
            event_root::RootError::Evaluation(e) => e,
            event_root::RootError::NonFiniteGuard => DynamicsError::NonFinite(self.time),
            event_root::RootError::InvalidBracket => DynamicsError::Schedule { time:self.time, reason:"invalid event crossing bracket" },
        })
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
                // Normally require the same rule and nearly identical step.
                // The event-search experiment permits a nearby step, but the
                // matrix remains stale: contraction/refresh checks still apply.
                let step_tolerance = if self.locating_event && self.event_jacobian_reuse {0.1} else {1.0e-4};
                let mut matrix_step = h;
                let mut cache = match self.newton_cache.take() {
                    Some((ch, ctheta, c)) if (ch - h).abs() <= step_tolerance * h && ctheta == theta => {
                        matrix_step=ch;
                        Some(c)
                    },
                    _ => None,
                };
                let mut result = profile::IMPLICIT.time(|| implicit_step(&self.system, t, h, x, config, &self.previous_rate, theta, &self.sparsity, &self.algebraic, self.use_provided_jacobian, None, &mut cache, depth,
                    (self.implicit_attempts.len() < self.attempt_limit).then_some(&mut self.implicit_attempts)));
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
                        result = profile::IMPLICIT.time(|| implicit_step(&self.system, t, h, x, config, &self.previous_rate, 1.0, &self.sparsity, &self.algebraic, self.use_provided_jacobian, Some(&branch), &mut None, depth,
                            (self.implicit_attempts.len() < self.attempt_limit).then_some(&mut self.implicit_attempts)));
                        if result.is_ok() {
                            self.stats.branch_restarts += 1;
                            break;
                        }
                    }
                }
                match result {
                    Ok((iterations, rebuilt)) => {
                        self.stats.max_newton_iterations = self.stats.max_newton_iterations.max(iterations);
                        if let Some(c) = cache {
                            // Keep the step at which the matrix was actually
                            // built, so successive small changes cannot drift
                            // beyond the reuse bound without a refresh.
                            self.newton_cache = Some((if rebuilt {h} else {matrix_step}, theta, c));
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
    use_provided_jacobian: bool,
    start: Option<&[f64]>,
    cache: &mut Option<JacobianCache>,
    subdivision_depth: u32,
    audit: Option<&mut Vec<ImplicitAttempt>>,
) -> Result<(usize, bool), DynamicsError> {
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
    let mut newton_audit = audit.as_ref().map(|_| sim_solve::NewtonAudit::default());
    let capture_linearization = audit.is_some();
    let mut last_linearization = None;
    let mut rebuilt = false;
    let diagnostics = solve_newton_cached_audited(&mut u, config, residual, |next, base, jacobian| {
        rebuilt = true;
        if capture_linearization {
            last_linearization = Some(ImplicitLinearization { increment:next.to_vec(), residual:base.to_vec() });
        }
        let mut mid = vec![0.0; n];
        let mut rate = vec![0.0; n];
        stage(next, &mut mid, &mut rate);
        parts.clear();
        if use_provided_jacobian && system.jacobian(t + theta * h, &mid, &rate, &mut parts) {
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
    }, &step_scale, cache, newton_audit.as_mut());
    if let Some(audit) = audit {
        let mut stage_state = vec![0.0; n];
        let mut stage_rate = vec![0.0; n];
        stage(&u, &mut stage_state, &mut stage_rate);
        let mut residual = vec![0.0; n];
        system.residual(t + theta * h, &stage_state, &stage_rate, &mut residual);
        audit.push(ImplicitAttempt { start_time:t, step:h, theta, stage_time:t + theta*h,
            subdivision_depth, branch:start.is_some(), solve_succeeded:diagnostics.is_ok(), committed:Some(false),
            error:diagnostics.as_ref().err().map(ToString::to_string), initial_state:old.clone(),
            stage_state, stage_rate, residual, newton:newton_audit.unwrap(), last_linearization });
    }
    let diagnostics = diagnostics.map_err(|source| DynamicsError::Solve { time: t, source })?;
    for i in 0..n {
        x[i] = old[i] + u[i];
    }
    Ok((diagnostics.iterations,rebuilt))
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

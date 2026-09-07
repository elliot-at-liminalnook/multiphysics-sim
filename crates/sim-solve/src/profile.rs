//! Coarse wall-clock tallies of where a run spends its time: residual
//! evaluations, Jacobian assembly, factorisation, back-substitution,
//! guards, event location and jumps. Off by default; [`enable`] turns the
//! timers on, [`report`] prints the table, [`reset`] zeroes it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use web_time::Instant;

static ENABLED: AtomicBool = AtomicBool::new(false);

pub struct Bucket {
    pub name: &'static str,
    nanos: AtomicU64,
    calls: AtomicU64,
}

impl Bucket {
    const fn new(name: &'static str) -> Self {
        Self { name, nanos: AtomicU64::new(0), calls: AtomicU64::new(0) }
    }
    #[inline]
    pub fn time<T>(&self, f: impl FnOnce() -> T) -> T {
        if !ENABLED.load(Relaxed) {
            return f();
        }
        let started = Instant::now();
        let out = f();
        self.nanos.fetch_add(started.elapsed().as_nanos() as u64, Relaxed);
        self.calls.fetch_add(1, Relaxed);
        out
    }
    #[inline]
    pub fn count(&self, n: u64) {
        if ENABLED.load(Relaxed) {
            self.calls.fetch_add(n, Relaxed);
        }
    }
    pub fn seconds(&self) -> f64 {
        self.nanos.load(Relaxed) as f64 * 1.0e-9
    }
    pub fn calls(&self) -> u64 {
        self.calls.load(Relaxed)
    }
    fn reset(&self) {
        self.nanos.store(0, Relaxed);
        self.calls.store(0, Relaxed);
    }
}

pub static STEP: Bucket = Bucket::new("step (total)");
pub static IMPLICIT: Bucket = Bucket::new("implicit solve");
pub static NEWTON: Bucket = Bucket::new("newton calls");
pub static ITERATIONS: Bucket = Bucket::new("newton iterations");
pub static FRESH: Bucket = Bucket::new("fresh jacobians");
/// Failed full trials with a reused matrix, refreshed without discarded backtracking probes.
pub static STALE_FULL_REFRESH: Bucket = Bucket::new("stale full-step refreshes");
pub static RESIDUAL: Bucket = Bucket::new("residual evaluations");
pub static JACOBIAN: Bucket = Bucket::new("jacobian assembly");
pub static ANALYTIC_SLOTS: Bucket = Bucket::new("  slots supplied");
/// These slots also count as FD slots because their state partials are differenced.
pub static RATE_SLOTS: Bucket = Bucket::new("  slots with supplied rate partials");
pub static FD_SLOTS: Bucket = Bucket::new("  slots finite-difference");
/// Component fallback probes, including their unperturbed baselines. Hybrid
/// implementations' internal probes are not included in this counter.
pub static FD_RESIDUALS: Bucket = Bucket::new("  component FD residual calls");
pub static FACTORISE: Bucket = Bucket::new("factorisation");
pub static FACTOR_SUM: Bucket = Bucket::new("  factor entries sum/sort");
pub static FACTOR_MATRIX: Bucket = Bucket::new("  sparse matrix construction");
pub static FACTOR_SYMBOLIC: Bucket = Bucket::new("  symbolic cache lookup/build");
pub static FACTOR_SYMBOLIC_BUILD: Bucket = Bucket::new("    symbolic cache misses");
pub static FACTOR_NUMERIC: Bucket = Bucket::new("  sparse numeric factorisation");
pub static SOLVE: Bucket = Bucket::new("back-substitution");
pub static GUARDS: Bucket = Bucket::new("guards");
pub static LOCATE: Bucket = Bucket::new("event location");
pub static JUMP: Bucket = Bucket::new("jumps (incl. coupler)");
pub static EMBEDDED_MAPPING: Bucket = Bucket::new("embedded closure mapping");
pub static EMBEDDED_HISTORY: Bucket = Bucket::new("embedded contact history");
pub static EMBEDDED_DYNAMICS_PREPARE: Bucket = Bucket::new("embedded dynamics preparation");
pub static EMBEDDED_DYNAMICS_APPLY: Bucket = Bucket::new("embedded applied force solve");
pub static EMBEDDED_COMPONENTS: Bucket = Bucket::new("embedded component equations");
pub static EMBEDDED_CLOSURE_JACOBIAN: Bucket = Bucket::new("  closure Jacobian");
pub static EMBEDDED_CLOSURE_FACTOR: Bucket = Bucket::new("  closure factorization");
pub static EMBEDDED_CLOSURE_SVD: Bucket = Bucket::new("    closure SVD");
pub static EMBEDDED_INERTIA: Bucket = Bucket::new("  embedded rigid inertia");
pub static EMBEDDED_PROJECT_INERTIA: Bucket = Bucket::new("  embedded inertia projection");
pub static EMBEDDED_FORCE_EVALUATION: Bucket = Bucket::new("  embedded force evaluation");
pub static CONTACT_GEOMETRY: Bucket = Bucket::new("contact geometry queries");
pub static CONTACT_FORCES: Bucket = Bucket::new("contact force evaluation");
pub static CONTACT_TOPOLOGY: Bucket = Bucket::new("  contact exclusion metadata");
pub static CONTACT_SAMPLES: Bucket = Bucket::new("  contact sample transforms");
pub static CONTACT_PAIRS: Bucket = Bucket::new("  contact pair queries");
pub static POLICY_REFERENCE: Bucket = Bucket::new("policy online reference");
pub static POLICY_OBSERVATIONS: Bucket = Bucket::new("policy observations");
pub static POLICY_BODY_FEEDBACK: Bucket = Bucket::new("policy body feedback");
pub static POLICY_POINT_FEEDBACK: Bucket = Bucket::new("policy point feedback");
pub static POLICY_SCRIPT: Bucket = Bucket::new("policy script");

pub fn all() -> [&'static Bucket; 43] {
    [&STEP, &IMPLICIT, &NEWTON, &ITERATIONS, &FRESH, &STALE_FULL_REFRESH, &RESIDUAL, &JACOBIAN, &ANALYTIC_SLOTS, &RATE_SLOTS, &FD_SLOTS, &FD_RESIDUALS, &FACTORISE, &FACTOR_SUM, &FACTOR_MATRIX, &FACTOR_SYMBOLIC, &FACTOR_SYMBOLIC_BUILD, &FACTOR_NUMERIC, &SOLVE, &GUARDS, &LOCATE, &JUMP,
        &EMBEDDED_MAPPING, &EMBEDDED_HISTORY, &EMBEDDED_DYNAMICS_PREPARE, &EMBEDDED_DYNAMICS_APPLY, &EMBEDDED_COMPONENTS,
        &EMBEDDED_CLOSURE_JACOBIAN, &EMBEDDED_CLOSURE_FACTOR, &EMBEDDED_CLOSURE_SVD,
        &EMBEDDED_INERTIA, &EMBEDDED_PROJECT_INERTIA, &EMBEDDED_FORCE_EVALUATION,
        &CONTACT_GEOMETRY, &CONTACT_FORCES, &CONTACT_TOPOLOGY, &CONTACT_SAMPLES, &CONTACT_PAIRS,
        &POLICY_REFERENCE, &POLICY_OBSERVATIONS, &POLICY_BODY_FEEDBACK, &POLICY_POINT_FEEDBACK, &POLICY_SCRIPT]
}

pub fn enable() {
    ENABLED.store(true, Relaxed);
}

pub fn reset() {
    all().iter().for_each(|b| b.reset());
}

/// The table, one bucket per line: seconds, calls, microseconds per call.
pub fn report() -> String {
    let mut out = String::new();
    let total = STEP.seconds().max(1.0e-12);
    out.push_str(&format!("{:<28}{:>10}{:>12}{:>12}{:>8}\n", "bucket", "seconds", "calls", "µs/call", "% step"));
    for b in all() {
        let s = b.seconds();
        let c = b.calls();
        let per = if c > 0 && s > 0.0 { format!("{:.1}", s * 1.0e6 / c as f64) } else { "-".into() };
        let pct = if s > 0.0 { format!("{:.0}", 100.0 * s / total) } else { "-".into() };
        out.push_str(&format!("{:<28}{:>10.4}{:>12}{:>12}{:>8}\n", b.name, s, c, per, pct));
    }
    out
}

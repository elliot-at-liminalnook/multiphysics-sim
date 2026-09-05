//! Coarse wall-clock tallies of where a run spends its time: residual
//! evaluations, Jacobian assembly, factorisation, back-substitution,
//! guards, event location and jumps. Off by default; [`enable`] turns the
//! timers on, [`report`] prints the table, [`reset`] zeroes it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

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
pub static RESIDUAL: Bucket = Bucket::new("residual evaluations");
pub static JACOBIAN: Bucket = Bucket::new("jacobian assembly");
pub static ANALYTIC_SLOTS: Bucket = Bucket::new("  slots analytic");
pub static FD_SLOTS: Bucket = Bucket::new("  slots finite-difference");
pub static FACTORISE: Bucket = Bucket::new("factorisation");
pub static SOLVE: Bucket = Bucket::new("back-substitution");
pub static GUARDS: Bucket = Bucket::new("guards");
pub static LOCATE: Bucket = Bucket::new("event location");
pub static JUMP: Bucket = Bucket::new("jumps (incl. coupler)");

pub fn all() -> [&'static Bucket; 14] {
    [&STEP, &IMPLICIT, &NEWTON, &ITERATIONS, &FRESH, &RESIDUAL, &JACOBIAN, &ANALYTIC_SLOTS, &FD_SLOTS, &FACTORISE, &SOLVE, &GUARDS, &LOCATE, &JUMP]
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

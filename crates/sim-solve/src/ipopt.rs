//! Opt-in native Ipopt C interface for sparse constrained nonlinear programs.
//! Rust owns callbacks and validation. No library is loaded by default.
use crate::least_squares::VariableBound;
use libloading::Library;
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CString, c_char, c_void},
    path::Path,
    ptr,
    sync::Mutex,
};

type P = *mut c_void;
type Eval = unsafe extern "C" fn(i32, *mut f64, bool, *mut f64, P) -> bool;
type EvalG = unsafe extern "C" fn(i32, *mut f64, bool, i32, *mut f64, P) -> bool;
type EvalJ =
    unsafe extern "C" fn(i32, *mut f64, bool, i32, i32, *mut i32, *mut i32, *mut f64, P) -> bool;
type EvalH = unsafe extern "C" fn(
    i32,
    *mut f64,
    bool,
    f64,
    i32,
    *mut f64,
    bool,
    i32,
    *mut i32,
    *mut i32,
    *mut f64,
    P,
) -> bool;
type Intermediate =
    unsafe extern "C" fn(i32, i32, f64, f64, f64, f64, f64, f64, f64, f64, i32, P) -> bool;
type Create = unsafe extern "C" fn(
    i32,
    *mut f64,
    *mut f64,
    i32,
    *mut f64,
    *mut f64,
    i32,
    i32,
    i32,
    Eval,
    EvalG,
    Eval,
    EvalJ,
    Option<EvalH>,
) -> P;
type Free = unsafe extern "C" fn(P);
type Solve =
    unsafe extern "C" fn(P, *mut f64, *mut f64, *mut f64, *mut f64, *mut f64, *mut f64, P) -> i32;

pub struct IpoptLibrary {
    _library: Library,
    create: Create,
    free: Free,
    solve: Solve,
    string_option: unsafe extern "C" fn(P, *mut c_char, *mut c_char) -> bool,
    number_option: unsafe extern "C" fn(P, *mut c_char, f64) -> bool,
    integer_option: unsafe extern "C" fn(P, *mut c_char, i32) -> bool,
    intermediate: unsafe extern "C" fn(P, Intermediate) -> bool,
    pub version: [i32; 3],
}
#[derive(Clone, Debug, Serialize)]
pub struct NlpEvaluation {
    pub objective: f64,
    pub constraints: Vec<f64>,
}
pub struct NlpDerivatives {
    pub objective_gradient: Vec<f64>,
    /// Values in the caller's fixed Jacobian sparsity order.
    pub constraint_jacobian: Vec<f64>,
}
pub trait NlpProblem {
    fn evaluate(&mut self, x: &[f64]) -> Result<NlpEvaluation, String>;
    fn derivatives(&mut self, x: &[f64]) -> Result<NlpDerivatives, String>;
    /// Return false to cancel. Must not re-enter the native solver.
    fn intermediate(&mut self, _iteration: &IpoptIteration) -> bool {
        true
    }
}
fn default_initial_bound_distance() -> f64 {
    0.01
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IpoptConfig {
    pub maximum_iterations: usize,
    /// Unique value/derivative callback requests plus initial/final evaluation.
    /// Does not count any numerical probes implemented inside a callback.
    pub maximum_callback_evaluations: usize,
    pub tolerance: f64,
    pub constraint_tolerance: f64,
    /// Ipopt's absolute interior push for initial variable values. This changes
    /// initialization only; bounds and physical constraints remain unchanged.
    #[serde(default = "default_initial_bound_distance")]
    pub initial_bound_push: f64,
    /// Relative interior distance, in (0, 0.5].
    #[serde(default = "default_initial_bound_distance")]
    pub initial_bound_fraction: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct IpoptIteration {
    pub restoration: bool,
    pub iteration: i32,
    pub objective: f64,
    pub primal_infeasibility: f64,
    pub dual_infeasibility: f64,
    pub barrier_parameter: f64,
    pub step_norm: f64,
}
#[derive(Debug, Serialize)]
pub struct IpoptResult {
    /// Raw ApplicationReturnStatus; feasibility is re-evaluated separately.
    pub native_status: i32,
    pub values: Vec<f64>,
    pub final_evaluation: Option<NlpEvaluation>,
    pub maximum_constraint_or_bound_violation: Option<f64>,
    pub value_evaluations: usize,
    pub derivative_evaluations: usize,
    pub rejected_callbacks: usize,
    pub budget_exhausted: bool,
    pub callback_panicked: bool,
    pub cancelled: bool,
    pub last_callback_error: Option<String>,
    pub iterations: Vec<IpoptIteration>,
}
struct Context<'a> {
    problem: &'a mut dyn NlpProblem,
    n: usize,
    m: usize,
    pattern: &'a [(usize, usize)],
    values: Option<(Vec<f64>, NlpEvaluation)>,
    derivatives: Option<(Vec<f64>, NlpDerivatives)>,
    value_evaluations: usize,
    derivative_evaluations: usize,
    rejected: usize,
    budget: usize,
    budget_exhausted: bool,
    panicked: bool,
    cancelled: bool,
    last_error: Option<String>,
    iterations: Vec<IpoptIteration>,
}
fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}
fn valid_evaluation(v: &NlpEvaluation, m: usize) -> bool {
    v.objective.is_finite()
        && v.constraints.len() == m
        && v.constraints.iter().all(|x| x.is_finite())
}
impl Context<'_> {
    fn request(&mut self) -> Result<(), String> {
        if self.panicked || self.cancelled {
            return Err("callback cancelled or panicked".into());
        }
        if self.value_evaluations + self.derivative_evaluations >= self.budget - 1 {
            self.budget_exhausted = true;
            return Err("callback evaluation budget exhausted".into());
        }
        Ok(())
    }
    fn values(&mut self, x: &[f64]) -> Result<&NlpEvaluation, String> {
        if !self.values.as_ref().is_some_and(|(old, _)| same(old, x)) {
            self.request()?;
            self.value_evaluations += 1;
            let v = self.problem.evaluate(x)?;
            if !valid_evaluation(&v, self.m) {
                return Err("invalid nonlinear value dimensions or nonfinite value".into());
            }
            self.values = Some((x.to_vec(), v));
        }
        Ok(&self.values.as_ref().unwrap().1)
    }
    fn derivatives(&mut self, x: &[f64]) -> Result<&NlpDerivatives, String> {
        if !self
            .derivatives
            .as_ref()
            .is_some_and(|(old, _)| same(old, x))
        {
            self.request()?;
            self.derivative_evaluations += 1;
            let d = self.problem.derivatives(x)?;
            if d.objective_gradient.len() != self.n
                || d.constraint_jacobian.len() != self.pattern.len()
                || !d
                    .objective_gradient
                    .iter()
                    .chain(&d.constraint_jacobian)
                    .all(|x| x.is_finite())
            {
                return Err("invalid nonlinear derivative dimensions or nonfinite value".into());
            }
            self.derivatives = Some((x.to_vec(), d));
        }
        Ok(&self.derivatives.as_ref().unwrap().1)
    }
}
// All callbacks are synchronous during solve, with this context alive and
// exclusively borrowed. Catch panics so Rust unwinding never crosses C.
unsafe fn callback(data: P, f: impl FnOnce(&mut Context<'_>) -> Result<(), String>) -> bool {
    if data.is_null() {
        return false;
    }
    let c = unsafe { &mut *data.cast::<Context<'_>>() };
    if c.panicked {
        return false;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(c))) {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            c.rejected += 1;
            c.last_error = Some(e);
            false
        }
        Err(_) => {
            c.rejected += 1;
            c.panicked = true;
            c.last_error = Some("Rust callback panicked".into());
            false
        }
    }
}
unsafe fn inputs<'a>(c: &Context<'_>, n: i32, x: *mut f64) -> Result<&'a [f64], String> {
    if n < 0 || n as usize != c.n || x.is_null() {
        return Err("invalid native input dimensions or pointer".into());
    }
    let x = unsafe { std::slice::from_raw_parts(x, c.n) };
    if !x.iter().all(|x| x.is_finite()) {
        return Err("nonfinite native input".into());
    }
    Ok(x)
}
unsafe extern "C" fn eval_f(n: i32, x: *mut f64, _: bool, out: *mut f64, data: P) -> bool {
    unsafe {
        callback(data, |c| {
            let x = inputs(c, n, x)?;
            if out.is_null() {
                return Err("null objective output".into());
            }
            *out = c.values(x)?.objective;
            Ok(())
        })
    }
}
unsafe extern "C" fn eval_grad(n: i32, x: *mut f64, _: bool, out: *mut f64, data: P) -> bool {
    unsafe {
        callback(data, |c| {
            let x = inputs(c, n, x)?;
            if out.is_null() {
                return Err("null gradient output".into());
            }
            let v = &c.derivatives(x)?.objective_gradient;
            ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
            Ok(())
        })
    }
}
unsafe extern "C" fn eval_g(n: i32, x: *mut f64, _: bool, m: i32, out: *mut f64, data: P) -> bool {
    unsafe {
        callback(data, |c| {
            let x = inputs(c, n, x)?;
            if m < 0 || m as usize != c.m || out.is_null() {
                return Err("invalid constraint output".into());
            }
            let v = &c.values(x)?.constraints;
            ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
            Ok(())
        })
    }
}
unsafe extern "C" fn eval_j(
    n: i32,
    x: *mut f64,
    _: bool,
    m: i32,
    nnz: i32,
    rows: *mut i32,
    cols: *mut i32,
    out: *mut f64,
    data: P,
) -> bool {
    unsafe {
        callback(data, |c| {
            if n < 0
                || n as usize != c.n
                || m < 0
                || m as usize != c.m
                || nnz < 0
                || nnz as usize != c.pattern.len()
            {
                return Err("invalid native Jacobian dimensions".into());
            }
            if out.is_null() {
                if rows.is_null() || cols.is_null() {
                    return Err("null Jacobian structure".into());
                }
                for (i, (r, k)) in c.pattern.iter().enumerate() {
                    *rows.add(i) = *r as i32;
                    *cols.add(i) = *k as i32;
                }
            } else {
                let x = inputs(c, n, x)?;
                let v = &c.derivatives(x)?.constraint_jacobian;
                ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
            }
            Ok(())
        })
    }
}
unsafe extern "C" fn intermediate(
    mode: i32,
    iteration: i32,
    objective: f64,
    primal: f64,
    dual: f64,
    mu: f64,
    step: f64,
    _: f64,
    _: f64,
    _: f64,
    _: i32,
    data: P,
) -> bool {
    unsafe {
        callback(data, |c| {
            if c.panicked || c.budget_exhausted || c.cancelled {
                return Err(
                    "solver stopping after callback failure, budget or cancellation".into(),
                );
            }
            let v = IpoptIteration {
                restoration: mode != 0,
                iteration,
                objective,
                primal_infeasibility: primal,
                dual_infeasibility: dual,
                barrier_parameter: mu,
                step_norm: step,
            };
            let keep = c.problem.intermediate(&v);
            c.iterations.push(v);
            if !keep {
                c.cancelled = true;
                return Err("cancelled by caller".into());
            }
            Ok(())
        })
    }
}
struct ProblemHandle(P, Free);
// The C adapter requires a non-null Hessian callback even in limited-memory
// mode. An unexpected invocation is an error, not a fabricated zero Hessian.
unsafe extern "C" fn unused_hessian(
    _: i32,
    _: *mut f64,
    _: bool,
    _: f64,
    _: i32,
    _: *mut f64,
    _: bool,
    _: i32,
    _: *mut i32,
    _: *mut i32,
    _: *mut f64,
    data: P,
) -> bool {
    unsafe {
        callback(data, |_| {
            Err("exact Hessian requested in limited-memory mode".into())
        })
    }
}
impl Drop for ProblemHandle {
    fn drop(&mut self) {
        unsafe { (self.1)(self.0) }
    }
}
static NATIVE_SOLVE: Mutex<()> = Mutex::new(());
impl IpoptLibrary {
    /// Load an explicitly selected native Ipopt 3.14.19 library.
    ///
    /// # Safety
    /// The caller must verify the library and its dependencies are trusted and
    /// its ABI uses 64-bit ipnumber, 32-bit ipindex and C bool, as in the recorded
    /// 3.14.19 build. Version alone cannot detect alternate integer/float builds.
    pub unsafe fn load_f64_i32_c_bool(path: &Path) -> Result<Self, String> {
        let lib = unsafe { Library::new(path) }.map_err(|e| e.to_string())?;
        let version_fn: unsafe extern "C" fn(*mut i32, *mut i32, *mut i32) =
            unsafe { *lib.get(b"GetIpoptVersion\0").map_err(|e| e.to_string())? };
        let mut version = [0; 3];
        unsafe {
            version_fn(&mut version[0], &mut version[1], &mut version[2]);
        }
        if version != [3, 14, 19] {
            return Err(format!(
                "unverified Ipopt ABI version {version:?}; expected 3.14.19"
            ));
        }
        unsafe {
            Ok(Self {
                create: *lib
                    .get(b"CreateIpoptProblem\0")
                    .map_err(|e| e.to_string())?,
                free: *lib.get(b"FreeIpoptProblem\0").map_err(|e| e.to_string())?,
                solve: *lib.get(b"IpoptSolve\0").map_err(|e| e.to_string())?,
                string_option: *lib.get(b"AddIpoptStrOption\0").map_err(|e| e.to_string())?,
                number_option: *lib.get(b"AddIpoptNumOption\0").map_err(|e| e.to_string())?,
                integer_option: *lib.get(b"AddIpoptIntOption\0").map_err(|e| e.to_string())?,
                intermediate: *lib
                    .get(b"SetIntermediateCallback\0")
                    .map_err(|e| e.to_string())?,
                _library: lib,
                version,
            })
        }
    }
    /// Sparse constrained NLP with a limited-memory Hessian approximation.
    /// Sparsity must include every entry that can become nonzero anywhere in
    /// the solve; inferring it from one numerical Jacobian is incorrect.
    /// Finite bounds have magnitude below 1e19. An infinite endpoint explicitly
    /// denotes an unbounded side. No option files or bound relaxation are used.
    pub fn solve(
        &self,
        initial: &[f64],
        bounds: &[VariableBound],
        constraint_bounds: &[VariableBound],
        pattern: &[(usize, usize)],
        config: &IpoptConfig,
        problem: &mut dyn NlpProblem,
    ) -> Result<IpoptResult, String> {
        let n = initial.len();
        let m = constraint_bounds.len();
        if n == 0
            || m == 0
            || n > i32::MAX as usize
            || m > i32::MAX as usize
            || pattern.is_empty()
            || pattern.len() > i32::MAX as usize
            || bounds.len() != n
            || bounds.iter().chain(constraint_bounds).any(|b| {
                b.lower.is_nan()
                    || b.upper.is_nan()
                    || b.lower > b.upper
                    || b.lower == f64::INFINITY
                    || b.upper == f64::NEG_INFINITY
                    || (b.lower.is_finite() && b.lower.abs() >= 1e19)
                    || (b.upper.is_finite() && b.upper.abs() >= 1e19)
            })
            || initial
                .iter()
                .zip(bounds)
                .any(|(x, b)| !x.is_finite() || *x < b.lower || *x > b.upper)
            || pattern.iter().any(|(r, c)| *r >= m || *c >= n)
            || pattern
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != pattern.len()
            || config.maximum_iterations == 0
            || config.maximum_iterations > i32::MAX as usize
            || config.maximum_callback_evaluations < 3
            || config.maximum_callback_evaluations > 10_000_000
            || !config.tolerance.is_finite()
            || config.tolerance <= 0.
            || !config.constraint_tolerance.is_finite()
            || config.constraint_tolerance <= 0.
            || !config.initial_bound_push.is_finite()
            || config.initial_bound_push <= 0.
            || !config.initial_bound_fraction.is_finite()
            || config.initial_bound_fraction <= 0.
            || config.initial_bound_fraction > 0.5
        {
            return Err(
                "invalid native NLP dimensions, bounds, sparsity or search configuration".into(),
            );
        }
        let _lock = NATIVE_SOLVE
            .try_lock()
            .map_err(|_| "native Ipopt solve already active or re-entered")?;
        let first = problem.evaluate(initial)?;
        if !valid_evaluation(&first, m) {
            return Err("invalid initial NLP evaluation".into());
        }
        let mut c = Context {
            problem,
            n,
            m,
            pattern,
            values: Some((initial.to_vec(), first)),
            derivatives: None,
            value_evaluations: 1,
            derivative_evaluations: 0,
            rejected: 0,
            budget: config.maximum_callback_evaluations,
            budget_exhausted: false,
            panicked: false,
            cancelled: false,
            last_error: None,
            iterations: vec![],
        };
        let native_bound = |x: f64| {
            if x.is_infinite() {
                x.signum() * 1e20
            } else {
                x
            }
        };
        let mut lower = bounds
            .iter()
            .map(|b| native_bound(b.lower))
            .collect::<Vec<_>>();
        let mut upper = bounds
            .iter()
            .map(|b| native_bound(b.upper))
            .collect::<Vec<_>>();
        let mut gl = constraint_bounds
            .iter()
            .map(|b| native_bound(b.lower))
            .collect::<Vec<_>>();
        let mut gu = constraint_bounds
            .iter()
            .map(|b| native_bound(b.upper))
            .collect::<Vec<_>>();
        let p = unsafe {
            (self.create)(
                n as i32,
                lower.as_mut_ptr(),
                upper.as_mut_ptr(),
                m as i32,
                gl.as_mut_ptr(),
                gu.as_mut_ptr(),
                pattern.len() as i32,
                0,
                0,
                eval_f,
                eval_g,
                eval_grad,
                eval_j,
                Some(unused_hessian),
            )
        };
        if p.is_null() {
            return Err("Ipopt problem creation failed".into());
        }
        let _handle = ProblemHandle(p, self.free);
        for (k, v) in [
            ("hessian_approximation", "limited-memory"),
            ("option_file_name", ""),
            ("nlp_scaling_method", "none"),
            ("linear_solver", "mumps"),
            ("sb", "yes"),
        ] {
            let k = CString::new(k).unwrap();
            let v = CString::new(v).unwrap();
            if !unsafe { (self.string_option)(p, k.as_ptr().cast_mut(), v.as_ptr().cast_mut()) } {
                return Err("Ipopt string option rejected".into());
            }
        }
        for (k, v) in [
            ("tol", config.tolerance),
            ("constr_viol_tol", config.constraint_tolerance),
            ("bound_relax_factor", 0.),
            ("bound_push", config.initial_bound_push),
            ("bound_frac", config.initial_bound_fraction),
        ] {
            let k = CString::new(k).unwrap();
            if !unsafe { (self.number_option)(p, k.as_ptr().cast_mut(), v) } {
                return Err("Ipopt number option rejected".into());
            }
        }
        for (k, v) in [
            ("max_iter", config.maximum_iterations as i32),
            ("acceptable_iter", 0),
            ("print_level", 0),
        ] {
            let k = CString::new(k).unwrap();
            if !unsafe { (self.integer_option)(p, k.as_ptr().cast_mut(), v) } {
                return Err("Ipopt integer option rejected".into());
            }
        }
        if !unsafe { (self.intermediate)(p, intermediate) } {
            return Err("Ipopt intermediate callback rejected".into());
        }
        let mut x = initial.to_vec();
        let mut g = vec![f64::NAN; m];
        let mut obj = f64::NAN;
        let status = unsafe {
            (self.solve)(
                p,
                x.as_mut_ptr(),
                g.as_mut_ptr(),
                &mut obj,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                (&mut c as *mut Context<'_>).cast(),
            )
        };
        // Always re-evaluate the returned point, never treat a native status as
        // a physical certificate. One callback was reserved for this purpose.
        let final_evaluation = if !c.panicked && x.iter().all(|v| v.is_finite()) {
            c.value_evaluations += 1;
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.problem.evaluate(&x)))
            {
                Ok(Ok(v)) if valid_evaluation(&v, m) => Some(v),
                Ok(Err(e)) => {
                    c.last_error = Some(e);
                    None
                }
                Ok(Ok(_)) => {
                    c.last_error = Some("invalid final NLP evaluation".into());
                    None
                }
                Err(_) => {
                    c.panicked = true;
                    c.last_error = Some("final evaluation panicked".into());
                    None
                }
            }
        } else {
            None
        };
        let maximum_constraint_or_bound_violation = final_evaluation.as_ref().map(|v| {
            x.iter()
                .zip(bounds)
                .chain(v.constraints.iter().zip(constraint_bounds))
                .fold(0.0_f64, |a, (x, b)| a.max(b.lower - x).max(x - b.upper))
        });
        Ok(IpoptResult {
            native_status: status,
            values: x,
            final_evaluation,
            maximum_constraint_or_bound_violation,
            value_evaluations: c.value_evaluations,
            derivative_evaluations: c.derivative_evaluations,
            rejected_callbacks: c.rejected,
            budget_exhausted: c.budget_exhausted,
            callback_panicked: c.panicked,
            cancelled: c.cancelled,
            last_callback_error: c.last_error,
            iterations: c.iterations,
        })
    }
}

//! Two-stage, second-order stiff integration through the existing residual solver.
use crate::{ImplicitAttempt, JacobianParts, System, implicit_step, jacobian::Sparsity};
use serde::Serialize;
use sim_solve::NewtonConfig;

/// SDIRK2 diagonal coefficient. The stability function is
/// `(1 + (1 - 2*gamma)*z) / (1 - gamma*z)^2`, tending to zero for stiff decay.
pub const GAMMA: f64 = 1.0 - std::f64::consts::FRAC_1_SQRT_2;

#[derive(Debug, Serialize)]
pub struct SdirkStep {
    pub time_s: f64,
    pub state: Vec<f64>,
    /// Each stage is a nonlinear equation solve, not an accepted physical substep.
    pub stages: Vec<ImplicitAttempt>,
}

#[derive(Debug, Serialize)]
pub struct SdirkFailure {
    pub message: String,
    pub stages: Vec<ImplicitAttempt>,
}
impl std::fmt::Display for SdirkFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for SdirkFailure {}
impl From<&str> for SdirkFailure {
    fn from(message: &str) -> Self {
        Self {
            message: message.into(),
            stages: vec![],
        }
    }
}
// Avoid floating-point reconstruction of a clock deadline inside the inner
// implicit solve. Both residual and analytic Jacobian see the exact stage time.
struct Stage<'a, S> {
    system: &'a S,
    time: f64,
}
impl<S: System> System for Stage<'_, S> {
    fn dimension(&self) -> usize {
        self.system.dimension()
    }
    fn residual(&self, _: f64, x: &[f64], v: &[f64], r: &mut [f64]) {
        self.system.residual(self.time, x, v, r)
    }
    fn jacobian(&self, _: f64, x: &[f64], v: &[f64], j: &mut JacobianParts) -> bool {
        self.system.jacobian(self.time, x, v, j)
    }
}

/// Solve Y1 = x + gamma*h*f(t+gamma*h,Y1), then
/// Y2 = x + (1-gamma)*h*k1 + gamma*h*f(t+h,Y2), with k1=(Y1-x)/(gamma*h).
/// The endpoint is Y2 (stiff accuracy). Algebraic unknowns are solved at each
/// stage; their fictitious rates never contribute to the second-stage anchor.
///
/// Uses the existing increment-scaled Newton/Jacobian and acceptance path.
/// No history crosses this call. The caller MUST hold external inputs/noise
/// fixed and split at known deadlines; this primitive neither samples a
/// controller nor locates contact events or retries/subdivides failed stages.
/// System callbacks must be pure. Input state is unchanged on every outcome.
/// This is a vector-state primitive; manifold coordinates need their own chart.
pub fn step<S: System>(
    system: &S,
    time: f64,
    h: f64,
    initial: &[f64],
    config: NewtonConfig,
) -> Result<SdirkStep, SdirkFailure> {
    let n = system.dimension();
    if !config.absolute_tolerance.is_finite()
        || config.absolute_tolerance <= 0.
        || !config.relative_tolerance.is_finite()
        || config.relative_tolerance < 0.
        || !config.min_line_search.is_finite()
        || config.min_line_search <= 0.
        || config.min_line_search > 1.
        || config.max_iterations == 0
        || config.max_iterations > 1000
    {
        return Err("invalid SDIRK2 Newton configuration".into());
    }
    if initial.len() != n || n == 0 || initial.iter().any(|x| !x.is_finite()) {
        return Err("SDIRK2 requires a finite state matching the system dimension".into());
    }
    if !time.is_finite()
        || time < 0.0
        || !h.is_finite()
        || h <= 0.0
        || !(time + h).is_finite()
        || time + GAMMA * h <= time
        || time + GAMMA * h >= time + h
    {
        return Err("SDIRK2 requires representable positive stage durations".into());
    }
    let algebraic = system.algebraic().unwrap_or_else(|| vec![false; n]);
    let sparsity = system
        .sparsity()
        .unwrap_or_else(|| Sparsity::new(vec![(0..n).collect(); n]));
    if algebraic.len() != n
        || sparsity.rows.len() != n
        || sparsity.rows.iter().flatten().any(|r| *r >= n)
    {
        return Err("SDIRK2 system layout does not match its dimension".into());
    }
    let mut stages = vec![];
    let mut first = initial.to_vec();
    let t1 = time + GAMMA * h;
    let result = implicit_step(
        &Stage { system, time: t1 },
        time,
        GAMMA * h,
        &mut first,
        config,
        &vec![0.; n],
        1.,
        &sparsity,
        &algebraic,
        true,
        Some(initial),
        &mut None,
        0,
        Some(&mut stages),
    );
    if let Some(stage) = stages.last_mut() {
        stage.branch = false;
        stage.stage_time = t1;
    }
    if let Err(error) = result {
        return Err(SdirkFailure {
            message: error.to_string(),
            stages,
        });
    }
    let mut anchor = initial.to_vec();
    for i in 0..n {
        anchor[i] = if algebraic[i] {
            first[i]
        } else {
            initial[i] + (1. - GAMMA) / GAMMA * (first[i] - initial[i])
        };
    }
    let mut end = anchor;
    // This is an affine stage anchor, not a physical state at this start time.
    // Supplying Y1 as the guess avoids evaluating a derivative at that anchor.
    let result = implicit_step(
        &Stage {
            system,
            time: time + h,
        },
        time + (1. - GAMMA) * h,
        GAMMA * h,
        &mut end,
        config,
        &vec![0.; n],
        1.,
        &sparsity,
        &algebraic,
        true,
        Some(&first),
        &mut None,
        0,
        Some(&mut stages),
    );
    if let Some(stage) = stages.last_mut() {
        stage.branch = false;
        stage.stage_time = time + h;
    }
    if let Err(error) = result {
        return Err(SdirkFailure {
            message: error.to_string(),
            stages,
        });
    }
    if end.iter().any(|x| !x.is_finite()) {
        return Err(SdirkFailure {
            message: "nonfinite SDIRK2 endpoint".into(),
            stages,
        });
    }
    Ok(SdirkStep {
        time_s: time + h,
        state: end,
        stages,
    })
}

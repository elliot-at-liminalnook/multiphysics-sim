//! Diagnostic re-solves of captured discrete equations, using the production
//! implicit step. These do not advance a simulation or choose a physical root.
use crate::{ImplicitAttempt, System, implicit_step, jacobian::Sparsity};
use serde::Serialize;
use sim_solve::NewtonConfig;

/// One full step and two half steps from exactly the same initial state and
/// held system context. All solves (including failures) are retained.
#[derive(Debug, Serialize)]
pub struct StepDoubling {
    pub coarse: ImplicitAttempt,
    /// True when the captured initial state matches bit for bit and its
    /// already-checked coarse result was reused instead of solved again.
    pub coarse_reused: bool,
    pub fine: Vec<ImplicitAttempt>,
    /// Fine endpoint minus coarse endpoint, for all coordinates. None unless
    /// all three solves succeeded. Algebraic values are observable differences,
    /// not differential-state local error estimates.
    pub endpoint_difference: Option<Vec<f64>>,
    /// Estimated error magnitude of the TWO-half-step endpoint, using
    /// |fine-coarse|/(2^p-1). Algebraic coordinates are None. The estimate only
    /// has its stated interpretation in a smooth, asymptotic regime; contact
    /// switches, multiple roots and unresolved stiffness can invalidate it.
    pub estimated_fine_error: Option<Vec<Option<f64>>>,
    pub method_order: u32,
}

/// Audit step doubling with the production implicit step, without accepting a
/// timestep or changing a simulation. Reuses the captured coarse result when
/// the initial state matches bit for bit; otherwise re-solves it. Only backward Euler and implicit midpoint
/// have a supported order estimate. Each path uses fresh matrices, not a shared
/// mutable cache. Preserves the same external-context restrictions as
/// [`refine_implicit_attempt`]. No tolerance or acceptance policy is imposed.
pub fn check_implicit_step_doubling<S: System>(
    system: &S,
    point: &ImplicitAttempt,
    initial_state: &[f64],
    config: NewtonConfig,
) -> Result<StepDoubling, String> {
    let order = if point.theta == 1.0 {
        1
    } else if point.theta == 0.5 {
        2
    } else {
        return Err("step doubling supports backward Euler and implicit midpoint".into());
    };
    let algebraic = validate(system, point, initial_state, config)?;
    // A captured failure remains a failure. A successful capture is reusable
    // only when it also satisfies the requested absolute residual floor;
    // tighter requested tolerances must not inherit an unchecked old success.
    let coarse_reused = initial_state
        .iter()
        .zip(&point.initial_state)
        .all(|(a, b)| a.to_bits() == b.to_bits())
        && (!point.solve_succeeded
            || point
                .residual
                .iter()
                .all(|r| r.abs() <= config.absolute_tolerance));
    let coarse = if coarse_reused {
        point.clone()
    } else {
        resolve_implicit_attempt(system, point, initial_state, config)?
    };
    let fine = refine_implicit_attempt(system, &coarse, initial_state, config, 2)?;
    let (difference, estimate) =
        if coarse.solve_succeeded && fine.len() == 2 && fine.iter().all(|p| p.solve_succeeded) {
            let endpoint = |p: &ImplicitAttempt, i: usize| {
                if algebraic[i] || p.theta == 1.0 {
                    p.stage_state[i]
                } else {
                    p.initial_state[i] + p.step * p.stage_rate[i]
                }
            };
            let delta: Vec<_> = (0..system.dimension())
                .map(|i| endpoint(&fine[1], i) - endpoint(&coarse, i))
                .collect();
            if delta.iter().any(|x| !x.is_finite()) {
                return Err("nonfinite endpoint difference".into());
            }
            let divisor = (2_u32.pow(order) - 1) as f64;
            let estimate = delta
                .iter()
                .enumerate()
                .map(|(i, d)| (!algebraic[i]).then_some(d.abs() / divisor))
                .collect();
            (Some(delta), Some(estimate))
        } else {
            (None, None)
        };
    Ok(StepDoubling {
        coarse,
        coarse_reused,
        fine,
        endpoint_difference: difference,
        estimated_fine_error: estimate,
        method_order: order,
    })
}

/// Re-solve one captured step from its terminal stage as a starting guess,
/// optionally replacing its initial state with a common comparison state.
/// The original terminal residual must reproduce bit for bit in `system`.
/// The caller must preserve external inputs, noise and the coordinate contract;
/// this check is necessary but does not reconstruct hidden history.
///
/// Uses the production increment mapping, Jacobian, scaling and acceptance
/// rules, with a fresh matrix and no subdivisions, events or branch retries.
/// A failed Newton solve is returned as a failed attempt, not discarded.
/// Distinct converged results are evidence about discrete roots, not proof of
/// continuous-time accuracy or a prescription for selecting a root.
pub fn resolve_implicit_attempt<S: System>(
    system: &S,
    point: &ImplicitAttempt,
    initial_state: &[f64],
    config: NewtonConfig,
) -> Result<ImplicitAttempt, String> {
    let algebraic = validate(system, point, initial_state, config)?;
    let n = system.dimension();
    // implicit_step accepts an END-state guess; preserve the captured STAGE
    // when mapping it to a different initial state (including midpoint DAEs).
    let start: Vec<_> = (0..n)
        .map(|i| {
            initial_state[i]
                + (point.stage_state[i] - initial_state[i])
                    / if algebraic[i] { 1.0 } else { point.theta }
        })
        .collect();
    if start.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite reconstructed end-state guess".into());
    }
    let sparsity = system
        .sparsity()
        .unwrap_or_else(|| Sparsity::new((0..n).map(|_| (0..n).collect()).collect()));
    let mut state = initial_state.to_vec();
    let mut audit = Vec::with_capacity(1);
    let _result = implicit_step(
        system,
        point.start_time,
        point.step,
        &mut state,
        config,
        &vec![0.0; n],
        point.theta,
        &sparsity,
        &algebraic,
        true,
        Some(&start),
        &mut None,
        0,
        Some(&mut audit),
    );
    audit
        .pop()
        .ok_or_else(|| "implicit re-solve did not record an attempt".into())
}

/// Reintegrate the captured interval with 1..=4096 equal subdivisions and held
/// external context. Uses the same method and production step, with fresh
/// matrices per substep and no event processing or automatic subdivision.
/// For one substep this is exactly [`resolve_implicit_attempt`]. Otherwise the
/// first predictor uses the captured rate; subsequent predictors use the last
/// solved rate. Algebraic starting guesses use captured values, then the last
/// solution. Returns every trial up to and including the first failed solve.
/// Inspect length and success before treating the interval as completed.
/// This is unsuitable across external input changes, noise draws or jumps unless
/// the caller separately splits the interval and restores that context.
pub fn refine_implicit_attempt<S: System>(
    system: &S,
    point: &ImplicitAttempt,
    initial_state: &[f64],
    config: NewtonConfig,
    substeps: usize,
) -> Result<Vec<ImplicitAttempt>, String> {
    if substeps == 0 || substeps > 4096 {
        return Err("substeps must be in 1..=4096".into());
    }
    if substeps == 1 {
        return Ok(vec![resolve_implicit_attempt(
            system,
            point,
            initial_state,
            config,
        )?]);
    }
    let algebraic = validate(system, point, initial_state, config)?;
    let n = system.dimension();
    let sparsity = system
        .sparsity()
        .unwrap_or_else(|| Sparsity::new((0..n).map(|_| (0..n).collect()).collect()));
    let mut state = initial_state.to_vec();
    let mut previous_rate = point.stage_rate.clone();
    let mut audit = Vec::with_capacity(substeps);
    let mut time = point.start_time;
    for k in 0..substeps {
        let end = point.start_time + point.step * ((k + 1) as f64 / substeps as f64);
        let h = end - time;
        if !end.is_finite() || h <= 0.0 {
            return Err("substep cannot be represented at captured time".into());
        }
        let start: Vec<_> = (0..n)
            .map(|i| {
                if algebraic[i] {
                    if k == 0 {
                        point.stage_state[i]
                    } else {
                        state[i]
                    }
                } else {
                    state[i] + h * previous_rate[i]
                }
            })
            .collect();
        if start.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite substep predictor".into());
        }
        let result = implicit_step(
            system,
            time,
            h,
            &mut state,
            config,
            &previous_rate,
            point.theta,
            &sparsity,
            &algebraic,
            true,
            Some(&start),
            &mut None,
            0,
            Some(&mut audit),
        );
        if result.is_err() {
            break;
        }
        previous_rate.copy_from_slice(&audit.last().unwrap().stage_rate);
        time = end;
    }
    Ok(audit)
}

fn validate<S: System>(
    system: &S,
    point: &ImplicitAttempt,
    initial_state: &[f64],
    config: NewtonConfig,
) -> Result<Vec<bool>, String> {
    let n = system.dimension();
    if n == 0
        || [
            initial_state,
            &point.initial_state,
            &point.stage_state,
            &point.stage_rate,
            &point.residual,
        ]
        .iter()
        .any(|v| v.len() != n || v.iter().any(|x| !x.is_finite()))
        || !point.start_time.is_finite()
        || !point.step.is_finite()
        || point.step <= 0.0
        || !point.theta.is_finite()
        || point.theta <= 0.0
        || point.theta > 1.0
        || !point.stage_time.is_finite()
        || point.stage_time != point.start_time + point.theta * point.step
        || !config.absolute_tolerance.is_finite()
        || config.absolute_tolerance <= 0.0
        || !config.relative_tolerance.is_finite()
        || config.relative_tolerance < 0.0
        || config.max_iterations == 0
        || !config.min_line_search.is_finite()
        || config.min_line_search <= 0.0
        || config.min_line_search > 1.0
    {
        return Err("invalid captured step, initial state or Newton configuration".into());
    }
    let algebraic = system.algebraic().unwrap_or_else(|| vec![false; n]);
    if algebraic.len() != n {
        return Err("algebraic dimension mismatch".into());
    }
    for i in 0..n {
        let increment = point.step * point.stage_rate[i];
        let stage_increment = if algebraic[i] {
            increment
        } else {
            point.theta * increment
        };
        let expected = point.initial_state[i] + stage_increment;
        let roundoff = 8.0
            * f64::EPSILON
            * (1.0
                + point.initial_state[i].abs()
                + point.stage_state[i].abs()
                + stage_increment.abs());
        if !expected.is_finite()
            || !roundoff.is_finite()
            || (expected - point.stage_state[i]).abs() > roundoff
        {
            return Err("captured state/rate does not satisfy the implicit stage mapping".into());
        }
    }
    let mut residual = vec![0.0; n];
    system.residual(
        point.stage_time,
        &point.stage_state,
        &point.stage_rate,
        &mut residual,
    );
    if residual
        .iter()
        .zip(&point.residual)
        .any(|(a, b)| a.to_bits() != b.to_bits())
    {
        return Err("recorded terminal residual does not match current system context".into());
    }
    Ok(algebraic)
}

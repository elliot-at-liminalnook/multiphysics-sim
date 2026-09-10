//! Experimental backward Euler in independent velocities. Geometry, closure,
//! inertia and loads are evaluated at each trial endpoint, not frozen. This is
//! a correctness baseline with optional exact mechanical-endpoint reuse.
use super::{EmbeddedAcceleration, EmbeddedMotion, Generalized, RigidEmbedding};
use crate::math::{V, quat, quat_parts};
use nalgebra::UnitQuaternion;
use sim_solve::{
    BlockDiagonalColoring, solve_newton_numeric_colored_scaled_audited,
    solve_newton_numeric_scaled_cached_audited,
};
use sim_solve::{
    JacobianCache, NewtonAudit, NewtonConfig, SolveDiagnostics, solve_newton_numeric_cached_audited,
    solve_newton_cached_audited_with_reference,
    solve_newton_numeric_scaled_cached_audited_with_reference,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImplicitStepConfig {
    /// Experimental two-stage stiff mechanical integration. Only the pure
    /// mechanical advancement adapter supports this; coupled motor auxiliary
    /// states must use their existing integration path. Default is backward Euler.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sdirk2: bool,
    /// Newton residuals are velocity increments divided by the embedding's
    /// declared length/angular scales (and a one-second velocity scale).
    pub newton: NewtonConfig,
    /// Absolute check of z_new-z_old-h*original_zdot in metres/radians.
    pub contact_history_tolerance: f64,
    /// Reuse the last mapped mechanical endpoint only for bitwise-identical
    /// mechanical unknowns within this solve. Component forces remain fresh.
    pub reuse_mechanical_endpoint: bool,
    /// Additionally reuse inertia/passive-load preparation at that exact
    /// endpoint. Requires reuse_mechanical_endpoint. Applied loads stay fresh.
    pub reuse_mechanical_dynamics: bool,
    /// Experimental modified Newton across accepted steps. Requires a workspace
    /// through a cached motor or mechanical advancement API; ordinary APIs start
    /// a fresh workspace.
    pub reuse_step_jacobian: bool,
    /// Opt-in guarded matrix reuse across controller events whose adapter
    /// explicitly declares an unchanged continuous equation structure.
    /// Motor-mode events still invalidate. Requires reuse_step_jacobian.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reuse_controller_sample_jacobian: bool,
    /// Pure mechanical advancement only: after a failed cached trial, restart
    /// once from the original state with fresh derivatives before subdivision.
    /// Failed work remains diagnostic; no acceptance tolerance changes.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub restart_failed_reused_mechanics: bool,
    /// Optional iteration cap for the first cached mechanical attempt. Requires
    /// fresh restart; that restart retains the original Newton iteration limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_mechanical_iteration_limit: Option<usize>,
    /// Collect iteration and accepted endpoint diagnostics for trials starting
    /// in this inclusive simulation-time window. On failure, append a bounded convergence tail
    /// to the error. Observational only; absent by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newton_audit_window_s: Option<[f64; 2]>,
    /// Experimental auxiliary rates as Newton unknowns, with endpoint states
    /// formed as old + h*rate. Callbacks using with_rates receive those rates
    /// directly, avoiding cancellation in (new-old)/h on short event steps.
    /// Original component residual units and tolerances remain unchanged.
    pub auxiliary_rate_unknowns: bool,
    /// Experimental nonlinear elimination: solve the original auxiliary
    /// equations at each trial mechanical endpoint, leaving only independent
    /// velocities in outer Newton. No independence between components is
    /// assumed. Inner solves are deterministic, trial-local, and use tighter
    /// tolerances; failure rejects the trial rather than freezing a load.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub condense_auxiliary: bool,
    /// Compress inner numerical derivatives only when the component adapter
    /// declares independent auxiliary blocks. Unknown adapters fall back to
    /// ordinary probes. Requires condense_auxiliary; no physical law changes.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub color_auxiliary_jacobian: bool,
    /// Express inner rate-coordinate corrections in endpoint-state units:
    /// x_new=x_old+h*rate, so a state scale s becomes s/h for rate corrections.
    /// Original residual bounds are unchanged. Requires condensed rate unknowns.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auxiliary_endpoint_correction_scale: bool,
    /// Experimental correction matrix: reuse one exact closure tangent and
    /// curvature only inside tiny numerical Jacobian probes. Ordinary residuals
    /// and accepted endpoints always solve full closure. Failed solves restart
    /// with exact numerical derivatives from the original state.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub linearized_jacobian_probes: bool,
    /// Dimensionless finite-difference step times (1 + |unknown|) for the
    /// approximate Jacobian only. Larger probes can reduce nested roundoff;
    /// ordinary exact derivatives keep the original 1e-6 step.
    #[serde(skip_serializing_if = "is_default_probe_step")]
    pub linearized_probe_relative_step: f64,
    /// Experimental temporal Newton guess from the last accepted velocity
    /// increment. Requires the cached-step path. Exact residuals must improve
    /// by 10%; failure retries original velocities with fresh exact derivatives.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub extrapolate_velocity_seed: bool,
    /// Reuse a bitwise-matching exact endpoint as a tangent Jacobian's base.
    /// Requires exact endpoint reuse and linearized Jacobian probes. Probe
    /// geometry is still cleared before ordinary residuals resume.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reuse_exact_probe_base: bool,
}
fn is_default_probe_step(value: &f64) -> bool {
    *value == 1e-6
}
impl Default for ImplicitStepConfig {
    fn default() -> Self {
        Self {
            sdirk2: false,
            newton: NewtonConfig {
                max_iterations: 40,
                min_line_search: 1.0 / 4096.0,
                reject_nonfinite_trials: true,
                ..NewtonConfig::default()
            },
            contact_history_tolerance: 1e-11,
            reuse_mechanical_endpoint: false,
            reuse_mechanical_dynamics: false,
            reuse_step_jacobian: false,
            reuse_controller_sample_jacobian: false,
            restart_failed_reused_mechanics: false,
            cached_mechanical_iteration_limit: None,
            newton_audit_window_s: None,
            auxiliary_rate_unknowns: false,
            condense_auxiliary: false,
            color_auxiliary_jacobian: false,
            auxiliary_endpoint_correction_scale: false,
            linearized_jacobian_probes: false,
            linearized_probe_relative_step: 1e-6,
            extrapolate_velocity_seed: false,
            reuse_exact_probe_base: false,
        }
    }
}

/// Caller-owned numerical workspace for one continuous model/trajectory.
/// Reuses correction matrices and optional initial-guess history; every accepted
/// physical endpoint is recomputed. Clear after
/// edits, external resets or changed force-law definitions. Cloning shares an
/// immutable factorization; trial refreshes cannot change another snapshot.
#[derive(Clone, Default)]
pub struct ImplicitSolverWorkspace {
    cache: Option<JacobianCache>,
    step_s: Option<f64>,
    end_time_s: Option<f64>,
    contacts: Vec<(usize, Option<usize>)>,
    auxiliary_rate_unknowns: Option<bool>,
    condense_auxiliary: Option<bool>,
    color_auxiliary_jacobian: Option<bool>,
    auxiliary_endpoint_correction_scale: Option<bool>,
    linearized_jacobian_probes: Option<bool>,
    linearized_probe_relative_step: Option<f64>,
    extrapolate_velocity_seed: Option<bool>,
    reuse_exact_probe_base: Option<bool>,
    // Accepted endpoint velocity and its increment, solely for the next guess.
    velocity_seed_history: Option<(Vec<f64>, Vec<f64>)>,
}
impl ImplicitSolverWorkspace {
    pub(super) fn has_jacobian(&self) -> bool {
        self.cache.is_some()
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ImplicitEndpointAudit {
    /// Times of this backward-Euler equation. In a multistage adapter its seed
    /// may be an affine anchor, not a physical intermediate trajectory state.
    pub equation_start_time_s: f64,
    pub equation_step_s: f64,
    pub seed_reduced_velocity: Vec<f64>,
    pub initial_reduced_velocity_guess: Vec<f64>,
    pub endpoint_reduced_velocity: Vec<f64>,
    pub seed_joint_positions: Vec<f64>,
    pub endpoint_joint_positions: Vec<f64>,
    /// Full model state layout, including floating poses and contact memory.
    pub seed_states: Vec<f64>,
    pub endpoint_states: Vec<f64>,
    pub endpoint_reduced_accelerations: Vec<f64>,
    pub endpoint_bristle_rates: Vec<f64>,
    pub newton: NewtonAudit,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ImplicitStepDiagnostics {
    /// Observational only, retained only in the configured Newton audit window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_audit: Option<ImplicitEndpointAudit>,
    pub nonlinear: SolveDiagnostics,
    /// Includes numerical derivative probes, backtracks and final verification.
    pub endpoint_evaluations: usize,
    pub started_with_reused_jacobian: bool,
    /// A rejected cached mechanical attempt preceding this successful fresh
    /// restart. Its work is excluded from the successful-attempt counters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fresh_restart_reason: Option<String>,
    pub mechanical_preparations: usize,
    pub mechanical_cache_hits: usize,
    pub dynamics_preparations: usize,
    pub dynamics_cache_hits: usize,
    pub auxiliary_solves: usize,
    pub auxiliary_evaluations: usize,
    pub auxiliary_newton_iterations: usize,
    pub colored_auxiliary_solves: usize,
    pub maximum_scaled_velocity_residual: f64,
    pub maximum_contact_history_residual: f64,
    pub maximum_auxiliary_residual: f64,
    /// Probe-only approximations; never counts an accepted physical endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linearized_probe_evaluations: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_jacobian_fallback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predicted_velocity_seed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reused_exact_probe_bases: Option<usize>,
}

#[derive(Debug)]
pub struct EmbeddedImplicitStep {
    pub time_s: f64,
    pub endpoint: EmbeddedAcceleration,
    pub diagnostics: ImplicitStepDiagnostics,
    /// Additional endpoint unknowns supplied through `step_implicit_coupled`.
    pub auxiliary: Vec<f64>,
}

/// A pure component evaluation coupled to the trial mechanical endpoint.
pub struct CoupledForces {
    /// Full generalized force/torque order, as in `rigid_mass_matrix`.
    pub generalized_loads: Vec<f64>,
    /// One equation per auxiliary unknown. The component adapter must include
    /// its time-discretization and declare deliberate equation scales. These
    /// are checked by the same Newton residual tolerances as mechanics.
    pub auxiliary_residuals: Vec<f64>,
}

impl RigidEmbedding<'_> {
    /// First-order backward Euler, including the original contact-memory and
    /// passive force laws. Independent velocities are the Newton unknowns;
    /// original geometric closure is enforced at every trial endpoint. Base
    /// orientation uses exp(h*world_omega_new)*orientation_old.
    ///
    /// At fixed pose/velocity the current shared contact law has independent
    /// affine memory equations zdot=drive-decay*z. Two simultaneous exact
    /// affine probes condense all those states as (z_old+h*drive)/(1+h*decay).
    /// Original endpoint derivatives verify every condensed update. This
    /// assumption must be revisited if the contact memory law changes.
    ///
    /// Loads MUST be pure position/velocity/history force laws, not functions
    /// of trial acceleration; controllers/events must be scheduled outside this
    /// method. No separate motor, thermal, firmware or IMU states are advanced.
    /// Failure never mutates the input. Convergence is not a timestep-accuracy
    /// guarantee, and this experimental method is not the runtime default.
    pub fn step_implicit<F>(
        &self,
        seed: &Generalized,
        time_s: f64,
        step_s: f64,
        config: &ImplicitStepConfig,
        loads: F,
    ) -> Result<EmbeddedImplicitStep, String>
    where
        F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
    {
        self.step_implicit_coupled(seed, &[], time_s, step_s, config, |t, _, g, _| {
            Ok(CoupledForces {
                generalized_loads: loads(t, g)?,
                auxiliary_residuals: vec![],
            })
        })
    }

    /// Solve the same mechanical backward-Euler equation with an explicit
    /// finite reduced-velocity Newton guess. The guess never changes the seed,
    /// residual, tolerances or accepted endpoint checks. It is useful when the
    /// equation seed is an affine anchor of a multistage method.
    pub fn step_implicit_with_velocity_guess<F>(
        &self,
        seed: &Generalized,
        time_s: f64,
        step_s: f64,
        config: &ImplicitStepConfig,
        velocity_guess: &[f64],
        loads: F,
    ) -> Result<EmbeddedImplicitStep, String>
    where
        F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
    {
        self.step_implicit_coupled_cached_with_guess(seed, &[], time_s, step_s,
            config, &mut ImplicitSolverWorkspace::default(), Some(velocity_guess), None, false,
            |t, _, g, _, _| Ok(CoupledForces {
                generalized_loads: loads(t, g)?, auxiliary_residuals: vec![],
            }))
    }

    /// Solve mechanics and additional component equations simultaneously.
    /// `coupling(end_time, step, trial_mechanics, trial_auxiliary)` MUST be pure
    /// and capture the previous component states immutably. It may use existing
    /// `Behavior::residual` implementations to supply discretized electrical,
    /// actuator or thermal equations and their forces, without duplicating laws.
    /// Auxiliary state ownership, units/scales and event scheduling belong to
    /// that adapter. No controller tick or accepted-state mutation may occur
    /// inside the callback. Both mechanics and auxiliary results commit only
    /// when the combined Newton solve passes; failure leaves both inputs intact.
    #[allow(clippy::too_many_arguments)]
    pub fn step_implicit_coupled<F>(
        &self,
        seed: &Generalized,
        auxiliary_seed: &[f64],
        time_s: f64,
        step_s: f64,
        config: &ImplicitStepConfig,
        coupling: F,
    ) -> Result<EmbeddedImplicitStep, String>
    where
        F: Fn(f64, f64, &Generalized, &[f64]) -> Result<CoupledForces, String>,
    {
        self.step_implicit_coupled_with_rates(
            seed,
            auxiliary_seed,
            time_s,
            step_s,
            config,
            |t, h, g, x, _| coupling(t, h, g, x),
        )
    }

    /// Coupled BE with explicit trial state rates. When auxiliary_rate_unknowns
    /// is enabled, use these rates in the component equations rather than
    /// reconstructing them by subtracting rounded endpoint states. The returned
    /// auxiliary vector still contains physical endpoint states, not rates.
    #[allow(clippy::too_many_arguments)]
    pub fn step_implicit_coupled_with_rates<F>(
        &self,
        seed: &Generalized,
        auxiliary_seed: &[f64],
        time_s: f64,
        step_s: f64,
        config: &ImplicitStepConfig,
        coupling: F,
    ) -> Result<EmbeddedImplicitStep, String>
    where
        F: Fn(f64, f64, &Generalized, &[f64], &[f64]) -> Result<CoupledForces, String>,
    {
        self.step_implicit_coupled_cached(
            seed,
            auxiliary_seed,
            time_s,
            step_s,
            config,
            &mut ImplicitSolverWorkspace::default(),
            None,
            false,
            coupling,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn step_implicit_coupled_cached<F>(
        &self,
        seed: &Generalized,
        auxiliary_seed: &[f64],
        time_s: f64,
        step_s: f64,
        config: &ImplicitStepConfig,
        workspace: &mut ImplicitSolverWorkspace,
        auxiliary_coloring: Option<&BlockDiagonalColoring>,
        scheduled_sensors: bool,
        coupling: F,
    ) -> Result<EmbeddedImplicitStep, String>
    where
        F: Fn(f64, f64, &Generalized, &[f64], &[f64]) -> Result<CoupledForces, String>,
    {
        self.step_implicit_coupled_cached_with_guess(seed, auxiliary_seed, time_s, step_s,
            config, workspace, None, auxiliary_coloring, scheduled_sensors, coupling)
    }

    #[allow(clippy::too_many_arguments)]
    fn step_implicit_coupled_cached_with_guess<F>(
        &self,
        seed: &Generalized,
        auxiliary_seed: &[f64],
        time_s: f64,
        step_s: f64,
        config: &ImplicitStepConfig,
        workspace: &mut ImplicitSolverWorkspace,
        mechanical_guess: Option<&[f64]>,
        auxiliary_coloring: Option<&BlockDiagonalColoring>,
        scheduled_sensors: bool,
        coupling: F,
    ) -> Result<EmbeddedImplicitStep, String>
    where
        F: Fn(f64, f64, &Generalized, &[f64], &[f64]) -> Result<CoupledForces, String>,
    {
        self.validate_seed(seed)?;
        if config.sdirk2 {
            return Err("SDIRK2 requires the pure mechanical advancement adapter".into());
        }
        if config.auxiliary_endpoint_correction_scale
            && !(config.condense_auxiliary && config.auxiliary_rate_unknowns)
        {
            return Err(
                "auxiliary endpoint correction scaling requires condensed rate unknowns".into(),
            );
        }
        if config.color_auxiliary_jacobian && !config.condense_auxiliary {
            return Err("auxiliary coloring requires auxiliary condensation".into());
        }
        if config.newton_audit_window_s.is_some_and(|[from, until]| {
            !from.is_finite() || !until.is_finite() || from < 0.0 || until < from
        }) {
            return Err("invalid Newton audit time window".into());
        }
        if config.reuse_controller_sample_jacobian && !config.reuse_step_jacobian {
            return Err("controller-sample reuse requires cross-step Jacobian reuse".into());
        }
        if config.extrapolate_velocity_seed && !config.reuse_step_jacobian {
            return Err("velocity seed extrapolation requires cross-step Jacobian reuse".into());
        }
        if config.reuse_exact_probe_base && !(config.reuse_mechanical_endpoint && config.linearized_jacobian_probes) {
            return Err("exact probe base reuse requires endpoint reuse and linearized Jacobian probes".into());
        }
        if config.reuse_mechanical_dynamics && !config.reuse_mechanical_endpoint {
            return Err("mechanical dynamics reuse requires exact endpoint reuse".into());
        }
        let nc = config.newton;
        if !time_s.is_finite()
            || time_s < 0.0
            || !step_s.is_finite()
            || step_s <= 0.0
            || !(time_s + step_s).is_finite()
            || time_s + step_s <= time_s
            || !config.contact_history_tolerance.is_finite()
            || config.contact_history_tolerance <= 0.0
            || !nc.absolute_tolerance.is_finite()
            || nc.absolute_tolerance <= 0.0
            || !nc.relative_tolerance.is_finite()
            || nc.relative_tolerance <= 0.0
            || !nc.min_line_search.is_finite()
            || nc.min_line_search <= 0.0
            || nc.min_line_search > 1.0
            || nc.max_iterations == 0
            || nc.max_iterations > 1000
            || !config.linearized_probe_relative_step.is_finite()
            || config.linearized_probe_relative_step <= 0.0
            || config.linearized_probe_relative_step > 1e-4
            || auxiliary_seed.iter().any(|v| !v.is_finite())
        {
            return Err("invalid implicit mechanical step configuration".into());
        }
        if !scheduled_sensors && !self.art.imus.is_empty() {
            return Err(
                "authored IMUs require scheduled motor or mechanical advancement".into(),
            );
        }
        let old_u = self.reduced_velocity(seed);
        let n = old_u.len();
        if mechanical_guess.is_some_and(|u| u.len() != n || u.iter().any(|v| !v.is_finite())) {
            return Err("invalid initial mechanical velocity guess".into());
        }
        let selected: Vec<_> = (0..self.base_columns)
            .chain(self.independent.iter().map(|i| self.base_columns + i))
            .collect();
        let evaluations = Cell::new(0);
        let preparations = Cell::new(0);
        let cache_hits = Cell::new(0);
        let dynamics_preparations = Cell::new(0);
        let dynamics_cache_hits = Cell::new(0);
        let auxiliary_solves = Cell::new(0);
        let auxiliary_evaluations = Cell::new(0);
        let auxiliary_iterations = Cell::new(0);
        let colored_auxiliary_solves = Cell::new(0);
        // Seed, step, time, map and configuration are immutable for this call.
        // Auxiliary values cannot affect mapping or the mechanical contact law.
        // Only mechanical state is cached here. The optional prepared dynamics
        // below also freezes its inertia and mechanical passive/contact loads;
        // fresh component forces are always applied. Neither cache crosses a
        // continuous solve, event jump, step change, or mutable history update.
        let cache: RefCell<Option<(Vec<u64>, Rc<EmbeddedMotion>)>> = RefCell::new(None);
        let dynamics_cache = RefCell::new(None);
        // Present only while a fresh Jacobian is assembled. Both endpoint
        // caches are cleared before/after probing, so probe geometry cannot escape.
        let probe_context: RefCell<Option<(Vec<f64>, Rc<EmbeddedMotion>)>> = RefCell::new(None);
        let linearized_probes = Cell::new(0usize);
        let reused_exact_probe_bases = Cell::new(0usize);
        let prepare = |u: &[f64]| -> Result<Rc<EmbeddedMotion>, String> {
            if config.reuse_mechanical_endpoint {
                if let Some((key, motion)) = cache.borrow().as_ref() {
                    if key.iter().copied().eq(u.iter().map(|v| v.to_bits())) {
                        cache_hits.set(cache_hits.get() + 1);
                        return Ok(Rc::clone(motion));
                    }
                }
            }
            preparations.set(preparations.get() + 1);
            let mut trial = seed.clone();
            for b in self.art.bases.iter().filter(|b| !b.grounded) {
                let s = b.state;
                for k in 0..3 {
                    trial.states[s + k] += step_s * u[k];
                }
                let p = &seed.states[s + 3..s + 7];
                let orientation = quat(p[0], p[1], p[2], p[3]);
                let rotation = UnitQuaternion::from_scaled_axis(V::new(u[3], u[4], u[5]) * step_s);
                trial.states[s + 3..s + 7].copy_from_slice(&quat_parts(&(rotation * orientation)));
            }
            let q: Vec<_> = self
                .independent
                .iter()
                .enumerate()
                .map(|(j, i)| seed.q[*i] + step_s * u[self.base_columns + j])
                .collect();
            let mut motion = if let Some((base_u, base)) = probe_context.borrow().as_ref() {
                let mut probe = (**base).clone();
                // Only the configured FD radius is allowed, with roundoff
                // headroom. Larger moves automatically use full closure.
                let small = u.iter().zip(base_u).all(|(a,b)| (a-b).abs() <= 2.0*config.linearized_probe_relative_step*(1.0+b.abs()));
                if small {
                    linearized_probes.set(linearized_probes.get()+1);
                    let delta = nalgebra::DVector::from_iterator(n, u.iter().zip(base_u).map(|(a,b)|step_s*(a-b)));
                    let full_delta = &base.tangent * delta;
                    for i in 0..probe.generalized.q.len() {
                        probe.generalized.q[i] += full_delta[self.base_columns+i];
                    }
                    for (&i, &value) in self.independent.iter().zip(&q) { probe.generalized.q[i] = value; }
                    for b in self.art.bases.iter().filter(|b| !b.grounded) {
                        probe.generalized.states[b.state..b.state+7].copy_from_slice(&trial.states[b.state..b.state+7]);
                    }
                    let velocity = &base.tangent * nalgebra::DVector::from_column_slice(u);
                    self.set_motion(&mut probe.generalized, velocity.as_slice(), base.acceleration_bias.as_slice());
                    // Report actual probe errors internally, without asserting
                    // that this approximate point satisfies full closure.
                    let rows = self.art.original_closure_values(&probe.generalized);
                    let error = |get: fn(&super::super::constraints::ClosureValues)->f64| rows.iter()
                        .map(|r|get(r).abs()/self.row_scale(&r.unit)).fold(0.0_f64,f64::max);
                    probe.maximum_scaled_position_error = error(|r|r.position);
                    probe.maximum_scaled_velocity_error = error(|r|r.velocity);
                    probe.maximum_scaled_acceleration_error = error(|r|r.acceleration);
                    probe
                } else { self.solve(&trial, &q, u)? }
            } else { self.solve(&trial, &q, u)? };
            if self.art.contact_on {
                sim_solve::profile::EMBEDDED_HISTORY.time(|| -> Result<(), String> {
                    let mut zero = motion.generalized.clone();
                    let mut one = zero.clone();
                    for link in self.art.links.iter().filter(|l| !l.grounded) {
                        zero.states[link.bristle_state..link.bristle_state + 3].fill(0.0);
                        one.states[link.bristle_state..link.bristle_state + 3].fill(1.0);
                    }
                    let evaluate = self.art.prepare_contact_history_rates(zero.clone());
                    let a = evaluate(&zero);
                    let b = evaluate(&one);
                    for link in self.art.links.iter().filter(|l| !l.grounded) {
                        for s in link.bristle_state..link.bristle_state + 3 {
                            let drive = a[s];
                            let decay = drive - b[s];
                            if !drive.is_finite() || !decay.is_finite() || decay < 0.0 {
                                return Err(
                                    "contact history is not a finite dissipative affine law".into(),
                                );
                            }
                            motion.generalized.states[s] =
                                (seed.states[s] + step_s * drive) / (1.0 + step_s * decay);
                        }
                    }
                    Ok(())
                })?;
            }
            let motion = Rc::new(motion);
            if config.reuse_mechanical_endpoint {
                *cache.borrow_mut() =
                    Some((u.iter().map(|v| v.to_bits()).collect(), Rc::clone(&motion)));
            }
            Ok(motion)
        };
        let auxiliary_state = |unknowns: &[f64]| -> Vec<f64> {
            if config.auxiliary_rate_unknowns {
                auxiliary_seed
                    .iter()
                    .zip(unknowns)
                    .map(|(old, rate)| old + step_s * rate)
                    .collect()
            } else {
                unknowns.to_vec()
            }
        };
        let endpoint =
            |unknowns: &[f64]| -> Result<(EmbeddedAcceleration, f64, Vec<f64>, Vec<f64>), String> {
                evaluations.set(evaluations.get() + 1);
                let motion = prepare(&unknowns[..n])?;
                let components_at = |x: &[f64]| -> Result<CoupledForces, String> {
                    let auxiliary = auxiliary_state(x);
                    let rates = if config.auxiliary_rate_unknowns {
                        x.to_vec()
                    } else {
                        auxiliary
                            .iter()
                            .zip(auxiliary_seed)
                            .map(|(new, old)| (new - old) / step_s)
                            .collect()
                    };
                    let components = sim_solve::profile::EMBEDDED_COMPONENTS.time(|| {
                        coupling(
                            time_s + step_s,
                            step_s,
                            &motion.generalized,
                            &auxiliary,
                            &rates,
                        )
                    })?;
                    if components.auxiliary_residuals.len() != auxiliary_seed.len()
                        || components
                            .auxiliary_residuals
                            .iter()
                            .any(|v| !v.is_finite())
                    {
                        return Err("invalid coupled auxiliary residuals".into());
                    }
                    Ok(components)
                };
                let mut local = if config.condense_auxiliary {
                    if config.auxiliary_rate_unknowns {
                        vec![0.0; auxiliary_seed.len()]
                    } else {
                        auxiliary_seed.to_vec()
                    }
                } else {
                    unknowns[n..].to_vec()
                };
                if config.condense_auxiliary && !local.is_empty() {
                    auxiliary_solves.set(auxiliary_solves.get() + 1);
                    let local_error = RefCell::new(None);
                    let inner = NewtonConfig {
                        absolute_tolerance: nc.absolute_tolerance * 0.01,
                        relative_tolerance: nc.relative_tolerance * 0.01,
                        ..nc
                    };
                    let correction_scale = |i: usize, value: f64| {
                        if config.auxiliary_endpoint_correction_scale {
                            (1.0 + (auxiliary_seed[i] + step_s * value).abs()) / step_s
                        } else {
                            1.0 + value.abs()
                        }
                    };
                    let inner_residual = |x: &[f64], r: &mut [f64]| {
                        auxiliary_evaluations.set(auxiliary_evaluations.get() + 1);
                        if config.auxiliary_endpoint_correction_scale
                            && x.iter()
                                .enumerate()
                                .any(|(i, v)| !correction_scale(i, *v).is_finite())
                        {
                            *local_error.borrow_mut() =
                                Some("nonfinite auxiliary correction scale".into());
                            r.fill(f64::NAN);
                            return;
                        }
                        match components_at(x) {
                            Ok(c) => r.copy_from_slice(&c.auxiliary_residuals),
                            Err(e) => {
                                *local_error.borrow_mut() = Some(e);
                                r.fill(f64::NAN);
                            }
                        }
                    };
                    let mut inner_audit = config
                        .newton_audit_window_s
                        .filter(|[from, until]| time_s >= *from && time_s <= *until)
                        .map(|_| NewtonAudit::default());
                    let solved = if let Some(coloring) =
                        auxiliary_coloring.filter(|_| config.color_auxiliary_jacobian)
                    {
                        colored_auxiliary_solves.set(colored_auxiliary_solves.get() + 1);
                        solve_newton_numeric_colored_scaled_audited(&mut local, inner, inner_residual, coloring, &correction_scale, inner_audit.as_mut())
                    } else {
                        solve_newton_numeric_scaled_cached_audited(&mut local, inner, inner_residual, &correction_scale, &mut None, inner_audit.as_mut())
                    }
                    .map_err(|e| {
                        let mut message=format!(
                            "local auxiliary solve: {e}; component error: {:?}",
                            local_error.borrow()
                        );
                        if let Some(audit)=&inner_audit {
                            use std::fmt::Write;
                            for entry in audit.iterations.iter().rev().take(2).rev() {
                                let correction=entry.correction.as_ref().and_then(|c|c.largest_unknowns.first());
                                let physical=correction.map(|(i,d,b,r)|(*i,if config.auxiliary_rate_unknowns {step_s*d} else {*d},if config.auxiliary_rate_unknowns {step_s*b} else {*b},*r));
                                let _=write!(message,"; inner audit [iteration={},fresh={},decision={},scaled_residual={:e},endpoint_correction(index,delta,bound,ratio)={physical:?}]",entry.iteration,entry.fresh_jacobian,entry.decision,entry.scaled_residual_norm);
                            }
                        }
                        message
                    })?;
                    auxiliary_iterations.set(auxiliary_iterations.get() + solved.iterations);
                }
                let components = components_at(&local)?;
                if config.condense_auxiliary {
                    auxiliary_evaluations.set(auxiliary_evaluations.get() + 1);
                    let error = components
                        .auxiliary_residuals
                        .iter()
                        .map(|v| v.abs())
                        .fold(0.0_f64, f64::max);
                    if error > nc.absolute_tolerance {
                        return Err(format!(
                            "local auxiliary original residual {error:e} exceeds {:e}",
                            nc.absolute_tolerance
                        ));
                    }
                }
                let a = if config.reuse_mechanical_dynamics {
                    let cached = {
                        let entry = dynamics_cache.borrow();
                        entry
                            .as_ref()
                            .filter(|(m, _)| Rc::ptr_eq(m, &motion))
                            .map(|(_, prepared)| Rc::clone(prepared))
                    };
                    let prepared = match cached {
                        Some(prepared) => {
                            dynamics_cache_hits.set(dynamics_cache_hits.get() + 1);
                            prepared
                        }
                        None => {
                            dynamics_preparations.set(dynamics_preparations.get() + 1);
                            let prepared = Rc::new(self.prepare_dynamics(&motion)?);
                            *dynamics_cache.borrow_mut() =
                                Some((Rc::clone(&motion), Rc::clone(&prepared)));
                            prepared
                        }
                    };
                    prepared.accelerations(&components.generalized_loads)?
                } else {
                    dynamics_preparations.set(dynamics_preparations.get() + 1);
                    self.accelerations(&motion, &components.generalized_loads)?
                };
                let mut history_error = 0.0_f64;
                for link in self.art.links.iter().filter(|l| !l.grounded) {
                    for s in link.bristle_state..link.bristle_state + 3 {
                        let r =
                            a.generalized.states[s] - seed.states[s] - step_s * a.bristle_rates[s];
                        if !r.is_finite() {
                            return Err("nonfinite implicit contact history".into());
                        }
                        history_error = history_error.max(r.abs());
                    }
                }
                if history_error > config.contact_history_tolerance {
                    return Err(format!(
                        "implicit original contact history residual {history_error:e}"
                    ));
                }
                Ok((
                    a,
                    history_error,
                    components.auxiliary_residuals,
                    auxiliary_state(&local),
                ))
            };
        let last_error = RefCell::new(None);
        let residual = |u: &[f64], r: &mut [f64]| match endpoint(u) {
            Ok((a, _, auxiliary, _)) => {
                for j in 0..n {
                    r[j] = (u[j] - old_u[j] - step_s * a.reduced_accelerations[j])
                        / self.column_scales[selected[j]];
                }
                if !config.condense_auxiliary {
                    r[n..].copy_from_slice(&auxiliary);
                }
            }
            Err(e) => {
                *last_error.borrow_mut() = Some(format!("{e}; trial reduced velocity {u:?}"));
                r.fill(f64::NAN);
            }
        };
        let mut u = mechanical_guess.map_or_else(|| old_u.clone(), |guess| guess.to_vec());
        if config.condense_auxiliary {
            // Auxiliary states are solved inside each mechanical trial.
        } else if config.auxiliary_rate_unknowns {
            u.resize(n + auxiliary_seed.len(), 0.0);
        } else {
            u.extend_from_slice(auxiliary_seed);
        }
        let mut next_workspace = workspace.clone();
        if !config.reuse_step_jacobian
            || next_workspace.auxiliary_rate_unknowns != Some(config.auxiliary_rate_unknowns)
            || next_workspace.condense_auxiliary != Some(config.condense_auxiliary)
            || next_workspace.color_auxiliary_jacobian != Some(config.color_auxiliary_jacobian)
            || next_workspace.auxiliary_endpoint_correction_scale
                != Some(config.auxiliary_endpoint_correction_scale)
            || next_workspace.linearized_jacobian_probes != Some(config.linearized_jacobian_probes)
            || next_workspace.linearized_probe_relative_step != Some(config.linearized_probe_relative_step)
            || next_workspace.extrapolate_velocity_seed != Some(config.extrapolate_velocity_seed)
            || next_workspace.reuse_exact_probe_base != Some(config.reuse_exact_probe_base)
            || next_workspace.velocity_seed_history.as_ref().is_some_and(|(velocity, increment)| {
                config.extrapolate_velocity_seed && (increment.len() != n || velocity.len() != n
                    || velocity.iter().zip(&old_u).any(|(a,b)|a.to_bits()!=b.to_bits()))
            })
            || !next_workspace
                .step_s
                .is_some_and(|h| (h - step_s).abs() <= 1e-10 * step_s)
            || !next_workspace
                .end_time_s
                .is_some_and(|t| (t - time_s).abs() <= 128.0 * f64::EPSILON * time_s.abs().max(1.0))
            || next_workspace.cache.as_ref().is_some_and(|c| c.uses >= 64)
        {
            next_workspace.clear();
        }
        let started_with_reused_jacobian = next_workspace.cache.is_some();
        let original_u = (config.linearized_jacobian_probes || config.extrapolate_velocity_seed).then(||u.clone());
        let mut predicted_velocity_seed = false;
        let mut original_residual = None;
        if config.extrapolate_velocity_seed {
            if let Some((_, increment)) = &next_workspace.velocity_seed_history {
                let mut candidate = u.clone();
                for j in 0..n { candidate[j] += increment[j]; }
                if candidate.iter().all(|v|v.is_finite()) {
                    let norm = |r:&[f64]| if r.iter().all(|v|v.is_finite()) {
                        r.iter().map(|v|v.abs()).fold(0.0_f64,f64::max)
                    } else {f64::INFINITY};
                    let mut r = vec![0.0;u.len()];
                    residual(&u,&mut r);
                    let original_norm = norm(&r);
                    if original_norm.is_finite() && original_norm > 0.0 {
                        let before_prediction = r.clone();
                        residual(&candidate,&mut r);
                        if norm(&r) < 0.9*original_norm {
                            u = candidate;
                            predicted_velocity_seed = true;
                            original_residual = Some(before_prediction);
                        }
                    }
                    *last_error.borrow_mut() = None;
                }
            }
        }
        let mut audit = config
            .newton_audit_window_s
            .filter(|[from, until]| time_s >= *from && time_s <= *until)
            .map(|_| NewtonAudit::default());
        let initial_reduced_velocity_guess = audit.as_ref().map(|_| u[..n].to_vec());
        let mut exact_jacobian_fallback = None;
        let nonlinear = if u.is_empty() {
            SolveDiagnostics {
                iterations: 0,
                residual_norm: 0.0,
                line_search_reductions: 0,
            }
        } else {
            let result = if config.linearized_jacobian_probes {
                let mut perturbed = vec![0.0; u.len()];
                solve_newton_cached_audited_with_reference(&mut u, nc, &residual, |x, base, jacobian| {
                    debug_assert!(probe_context.borrow().is_none());
                    if !config.reuse_exact_probe_base { *cache.borrow_mut() = None; }
                    *dynamics_cache.borrow_mut() = None;
                    let before = cache_hits.get();
                    let motion = prepare(&x[..n]);
                    if config.reuse_exact_probe_base { reused_exact_probe_bases.set(reused_exact_probe_bases.get()+cache_hits.get()-before); }
                    *cache.borrow_mut() = None;
                    *dynamics_cache.borrow_mut() = None;
                    match motion {
                        Ok(motion) => *probe_context.borrow_mut() = Some((x[..n].to_vec(),motion)),
                        Err(_) => { jacobian.add(0,0,f64::NAN); return; }
                    }
                    for column in 0..x.len() {
                        let original = x[column];
                        let epsilon = config.linearized_probe_relative_step*(1.0+original.abs());
                        x[column] = original+epsilon;
                        residual(x,&mut perturbed);
                        x[column] = original;
                        for row in 0..x.len() { jacobian.add(row,column,(perturbed[row]-base[row])/epsilon); }
                    }
                    *probe_context.borrow_mut() = None;
                    *cache.borrow_mut() = None;
                    *dynamics_cache.borrow_mut() = None;
                }, &|_,v|1.0+v.abs(), &mut next_workspace.cache, audit.as_mut(), original_residual.as_deref())
            } else {
                solve_newton_numeric_scaled_cached_audited_with_reference(&mut u, nc, &residual, &|_,v|1.0+v.abs(), &mut next_workspace.cache, audit.as_mut(), original_residual.as_deref())
            };
            let result = if (config.linearized_jacobian_probes || predicted_velocity_seed) && result.is_err() {
                exact_jacobian_fallback = Some(result.unwrap_err().to_string());
                u.clone_from(original_u.as_ref().expect("guarded solve original guess"));
                next_workspace.cache = None;
                *cache.borrow_mut() = None;
                *dynamics_cache.borrow_mut() = None;
                *last_error.borrow_mut() = None;
                if let Some(audit) = audit.as_mut() { *audit = NewtonAudit::default(); }
                solve_newton_numeric_cached_audited(&mut u, nc, &residual, &mut next_workspace.cache, audit.as_mut())
            } else { result };
            result.map_err(
                |e| {
                    // Re-evaluate the rejected final candidate to identify the
                    // failing equation, without committing state or accepting
                    // a looser tolerance. This extra work happens only on error.
                    let mut rejected = vec![0.0; u.len()];
                    residual(&u, &mut rejected);
                    let worst = rejected.iter().enumerate().max_by(|(_, a), (_, b)| {
                        a.abs().total_cmp(&b.abs())
                    }).map(|(row, value)| {
                        if row < n {
                            format!("mechanical velocity row {row}: residual={value:e}, candidate={:e}", u[row])
                        } else {
                            format!("auxiliary row {}: residual={value:e}, old={:e}, candidate={:e}", row-n, auxiliary_seed[row-n], u[row])
                        }
                    });
                    let mut message = format!(
                        "implicit mechanical solve: {e}; largest final residual: {worst:?}; last rejected endpoint: {:?}", last_error.borrow());
                    if let Some(audit) = &audit {
                        use std::fmt::Write;
                        let _ = write!(message, "; Newton tail [iteration, fresh, scaled residual, decision, largest normalized correction (index, delta, bound, ratio)]:");
                        for entry in audit.iterations.iter().rev().take(4).rev() {
                            let largest = entry.correction.as_ref().and_then(|c| c.largest_unknowns.first());
                            let _ = write!(message, " [{}, {}, {:.5e}, {}, {:?}]", entry.iteration, entry.fresh_jacobian, entry.scaled_residual_norm, entry.decision, largest);
                        }
                    }
                    message
                },
            )?
        };
        let (a, history_error, auxiliary, auxiliary_endpoint) = endpoint(&u)?;
        let velocity_error = (0..n)
            .map(|j| {
                ((u[j] - old_u[j] - step_s * a.reduced_accelerations[j])
                    / self.column_scales[selected[j]])
                    .abs()
            })
            .fold(0.0_f64, f64::max);
        if !velocity_error.is_finite() {
            return Err("nonfinite implicit endpoint residual".into());
        }
        let contacts: Vec<_> = a.contacts.iter().map(|c| (c.link, c.other)).collect();
        let changed_contacts = contacts != next_workspace.contacts;
        if changed_contacts {
            next_workspace.cache = None;
        }
        next_workspace.velocity_seed_history = if config.extrapolate_velocity_seed && !changed_contacts && exact_jacobian_fallback.is_none() {
            let velocity = self.reduced_velocity(&a.generalized);
            let increment = velocity.iter().zip(&old_u).map(|(a,b)|a-b).collect();
            Some((velocity,increment))
        } else { None };
        next_workspace.contacts = contacts;
        next_workspace.step_s = Some(step_s);
        next_workspace.end_time_s = Some(time_s + step_s);
        next_workspace.auxiliary_rate_unknowns = Some(config.auxiliary_rate_unknowns);
        next_workspace.condense_auxiliary = Some(config.condense_auxiliary);
        next_workspace.color_auxiliary_jacobian = Some(config.color_auxiliary_jacobian);
        next_workspace.auxiliary_endpoint_correction_scale =
            Some(config.auxiliary_endpoint_correction_scale);
        next_workspace.linearized_jacobian_probes = Some(config.linearized_jacobian_probes);
        next_workspace.linearized_probe_relative_step = Some(config.linearized_probe_relative_step);
        next_workspace.extrapolate_velocity_seed = Some(config.extrapolate_velocity_seed);
        next_workspace.reuse_exact_probe_base = Some(config.reuse_exact_probe_base);
        if !config.reuse_step_jacobian {
            next_workspace.clear();
        }
        *workspace = next_workspace;
        let endpoint_audit = audit.map(|newton| ImplicitEndpointAudit {
            equation_start_time_s: time_s,
            equation_step_s: step_s,
            seed_reduced_velocity: old_u,
            initial_reduced_velocity_guess: initial_reduced_velocity_guess.expect("enabled endpoint audit"),
            endpoint_reduced_velocity: self.reduced_velocity(&a.generalized),
            seed_joint_positions: seed.q.clone(),
            endpoint_joint_positions: a.generalized.q.clone(),
            seed_states: seed.states.clone(),
            endpoint_states: a.generalized.states.clone(),
            endpoint_reduced_accelerations: a.reduced_accelerations.as_slice().to_vec(),
            endpoint_bristle_rates: a.bristle_rates.clone(),
            newton,
        });
        Ok(EmbeddedImplicitStep {
            time_s: time_s + step_s,
            endpoint: a,
            diagnostics: ImplicitStepDiagnostics {
                endpoint_audit,
                nonlinear,
                endpoint_evaluations: evaluations.get(),
                started_with_reused_jacobian,
                fresh_restart_reason: None,
                mechanical_preparations: preparations.get(),
                mechanical_cache_hits: cache_hits.get(),
                dynamics_preparations: dynamics_preparations.get(),
                dynamics_cache_hits: dynamics_cache_hits.get(),
                auxiliary_solves: auxiliary_solves.get(),
                auxiliary_evaluations: auxiliary_evaluations.get(),
                auxiliary_newton_iterations: auxiliary_iterations.get(),
                colored_auxiliary_solves: colored_auxiliary_solves.get(),
                maximum_scaled_velocity_residual: velocity_error,
                maximum_contact_history_residual: history_error,
                maximum_auxiliary_residual: auxiliary.iter().map(|v| v.abs()).fold(0.0, f64::max),
                linearized_probe_evaluations: config.linearized_jacobian_probes.then_some(linearized_probes.get()),
                predicted_velocity_seed: config.extrapolate_velocity_seed.then_some(predicted_velocity_seed),
                reused_exact_probe_bases: config.reuse_exact_probe_base.then_some(reused_exact_probe_bases.get()),
                exact_jacobian_fallback,
            },
            auxiliary: auxiliary_endpoint,
        })
    }
}

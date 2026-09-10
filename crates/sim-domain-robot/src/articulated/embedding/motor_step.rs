//! Connect the shared hybrid scheduler to the reduced mechanism and registered
//! motor components. No robot-specific topology or gearbox law lives here.
use super::control::{ContinuousBoundaries, SampledMotorControl};
use super::{
    EmbeddedAcceleration, EmbeddedMotorBank, Generalized, ImplicitSolverWorkspace,
    ImplicitStepConfig, MotorBoundary, RigidEmbedding,
};
use sim_dynamics::hybrid::{HybridConfig, HybridDiagnostics, HybridStepper, advance_interval};
use std::cell::RefCell;
use std::sync::Arc;

/// Original contact-law sample at the accepted backward-Euler endpoint.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EmbeddedContactSample {
    pub link: usize,
    pub other: Option<usize>,
    pub force_n: [f64; 3],
    pub point_m: [f64; 3],
    pub penetration_m: f64,
}

/// Accepted continuous segment BEFORE a possible endpoint motor-mode jump.
/// F_endpoint * step_s is the BE contact impulse, not a continuous-time truth.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EmbeddedContactStep {
    pub start_time_s: f64,
    pub step_s: f64,
    pub contacts: Vec<EmbeddedContactSample>,
}

#[derive(Clone)]
struct ContactTrace {
    previous: Option<Arc<ContactTrace>>,
    step: EmbeddedContactStep,
}

impl Drop for ContactTrace {
    fn drop(&mut self) {
        // A large diagnostic interval must not recursively drop a long chain.
        let mut previous = self.previous.take();
        while let Some(node) = previous {
            match Arc::try_unwrap(node) {
                Ok(mut node) => previous = node.previous.take(),
                Err(_) => break,
            }
        }
    }
}

#[derive(Clone)]
struct MotorState<S> {
    held: S,
    workspace: ImplicitSolverWorkspace,
    mechanics: Generalized,
    auxiliary: Vec<f64>,
    // Persistent diagnostic chain: discarded candidates drop their own node.
    // No external logging occurs until the entire interval succeeds.
    contacts: Option<Arc<ContactTrace>>,
}

#[derive(Default, Clone, Debug, serde::Serialize)]
pub struct MotorSolveStatistics {
    /// Successful continuous trials, including discarded location trials.
    pub successful_trials: usize,
    pub successful_trials_with_reused_jacobian: usize,
    /// Work in successful trials only; failed-solve counts are reported by the
    /// hybrid scheduler but their internal residual counts are unavailable.
    pub successful_trial_endpoint_evaluations: usize,
    pub successful_trial_mechanical_preparations: usize,
    pub successful_trial_mechanical_cache_hits: usize,
    pub successful_trial_dynamics_preparations: usize,
    pub successful_trial_dynamics_cache_hits: usize,
    pub successful_trial_newton_iterations: usize,
    pub successful_trial_auxiliary_solves: usize,
    pub successful_trial_auxiliary_evaluations: usize,
    pub successful_trial_auxiliary_newton_iterations: usize,
    pub successful_trial_colored_auxiliary_solves: usize,
    pub maximum_scaled_velocity_residual: f64,
    pub maximum_auxiliary_residual: f64,
}

#[derive(Debug)]
pub struct EmbeddedMotorAdvance {
    pub time_s: f64,
    /// Instantaneous endpoint dynamics AFTER any final mode jumps.
    pub endpoint: EmbeddedAcceleration,
    pub motor_states: Vec<f64>,
    pub hybrid: HybridDiagnostics,
    pub solves: MotorSolveStatistics,
    /// None when disabled. Includes every accepted continuous segment, including
    /// segments with zero contacts; complete coverage can therefore be checked.
    pub contact_steps: Option<Vec<EmbeddedContactStep>>,
}

#[derive(Debug)]
pub struct EmbeddedControlledAdvance<S> {
    pub motor: EmbeddedMotorAdvance,
    pub control_state: S,
    pub control_guard_offset: usize,
}

struct MotorStepper<'a, 'm, F, C> {
    bank: &'a mut EmbeddedMotorBank,
    map: &'a RigidEmbedding<'m>,
    config: &'a ImplicitStepConfig,
    control: &'a mut C,
    external: &'a F,
    stats: RefCell<MotorSolveStatistics>,
}
impl<F, C> HybridStepper for MotorStepper<'_, '_, F, C>
where
    F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
    C: SampledMotorControl,
{
    type State = MotorState<C::State>;
    fn advance(
        &self,
        t: f64,
        h: f64,
        state: &MotorState<C::State>,
    ) -> Result<MotorState<C::State>, String> {
        let mut workspace = state.workspace.clone();
        let coloring = if self.config.color_auxiliary_jacobian
            && self.control.independent_motor_boundaries()
        {
            Some(sim_solve::BlockDiagonalColoring::new(
                &self
                    .bank
                    .state_layout()
                    .iter()
                    .map(|(_, n, _)| *n)
                    .collect::<Vec<_>>(),
            )?)
        } else {
            None
        };
        let step = self.map.step_implicit_coupled_cached(
            &state.mechanics,
            &state.auxiliary,
            t,
            h,
            self.config,
            &mut workspace,
            coloring.as_ref(),
            true,
            |t, _h, g, x, rates| {
                let boundaries = self.control.boundaries(t, g, x, &state.held)?;
                let mut result = self
                    .bank
                    .evaluate_with_rates(t, g, x, rates, &boundaries)?
                    .0;
                let extra = (self.external)(t, g)?;
                if extra.len() != result.generalized_loads.len()
                    || extra.iter().any(|v| !v.is_finite())
                {
                    return Err("invalid external loads for motor step".into());
                }
                for (f, e) in result.generalized_loads.iter_mut().zip(extra) {
                    *f += e;
                }
                Ok(result)
            },
        )?;
        let mut stats = self.stats.borrow_mut();
        stats.successful_trials += 1;
        stats.successful_trials_with_reused_jacobian +=
            usize::from(step.diagnostics.started_with_reused_jacobian);
        stats.successful_trial_endpoint_evaluations += step.diagnostics.endpoint_evaluations;
        stats.successful_trial_mechanical_preparations += step.diagnostics.mechanical_preparations;
        stats.successful_trial_mechanical_cache_hits += step.diagnostics.mechanical_cache_hits;
        stats.successful_trial_dynamics_preparations += step.diagnostics.dynamics_preparations;
        stats.successful_trial_dynamics_cache_hits += step.diagnostics.dynamics_cache_hits;
        stats.successful_trial_newton_iterations += step.diagnostics.nonlinear.iterations;
        stats.successful_trial_auxiliary_solves += step.diagnostics.auxiliary_solves;
        stats.successful_trial_auxiliary_evaluations += step.diagnostics.auxiliary_evaluations;
        stats.successful_trial_auxiliary_newton_iterations +=
            step.diagnostics.auxiliary_newton_iterations;
        stats.successful_trial_colored_auxiliary_solves +=
            step.diagnostics.colored_auxiliary_solves;
        stats.maximum_scaled_velocity_residual = stats
            .maximum_scaled_velocity_residual
            .max(step.diagnostics.maximum_scaled_velocity_residual);
        stats.maximum_auxiliary_residual = stats
            .maximum_auxiliary_residual
            .max(step.diagnostics.maximum_auxiliary_residual);
        let contacts = self.bank.audit_contact_steps.then(|| {
            Arc::new(ContactTrace {
                previous: state.contacts.clone(),
                step: EmbeddedContactStep {
                    start_time_s: t,
                    step_s: h,
                    contacts: step
                        .endpoint
                        .contacts
                        .iter()
                        .map(|c| EmbeddedContactSample {
                            link: c.link,
                            other: c.other,
                            force_n: std::array::from_fn(|i| c.force[i]),
                            point_m: std::array::from_fn(|i| c.point[i]),
                            penetration_m: c.penetration,
                        })
                        .collect(),
                },
            })
        });
        Ok(MotorState {
            held: state.held.clone(),
            workspace,
            contacts,
            mechanics: step.endpoint.generalized,
            auxiliary: step.auxiliary,
        })
    }
    fn guards(&self, t: f64, state: &MotorState<C::State>) -> Result<Vec<f64>, String> {
        let boundaries =
            self.control
                .boundaries(t, &state.mechanics, &state.auxiliary, &state.held)?;
        let mut guards = self
            .bank
            .event_data(t, &state.mechanics, &state.auxiliary, &boundaries)?
            .0;
        guards.extend(self.control.guards(t, &state.mechanics, &state.held)?);
        guards.extend(self.map.art.imus.iter().map(|imu| state.mechanics.states[imu.state + 15] - t));
        Ok(guards)
    }
    fn scheduled(&self, t: f64, state: &MotorState<C::State>) -> Result<Vec<(usize, f64)>, String> {
        let boundaries =
            self.control
                .boundaries(t, &state.mechanics, &state.auxiliary, &state.held)?;
        let mut deadlines = self
            .bank
            .event_data(t, &state.mechanics, &state.auxiliary, &boundaries)?
            .1;
        deadlines.extend(
            self.control
                .scheduled(t, &state.mechanics, &state.held)?
                .into_iter()
                .map(|(g, t)| (g + self.bank.guard_count(), t)),
        );
        let offset = self.bank.guard_count() + self.control.guards(t, &state.mechanics, &state.held)?.len();
        deadlines.extend(self.map.art.imus.iter().enumerate().map(|(i, imu)|
            (offset + i, state.mechanics.states[imu.state + 15])));
        Ok(deadlines)
    }
    fn jump(
        &mut self,
        guard: usize,
        t: f64,
        state: &mut MotorState<C::State>,
    ) -> Result<(), String> {
        let sensor_offset = self.bank.guard_count() + self.control.guards(t, &state.mechanics, &state.held)?.len();
        if guard >= sensor_offset {
            // Sensors are held states with no force feedback. The shared hybrid
            // scheduler lands exactly on their clocks and commits jumps atomically.
            return self.map.art.sample_imu_event(guard - sensor_offset, &mut state.mechanics);
        }
        let control_guard = guard.checked_sub(self.bank.guard_count());
        let keep_proposal = self.config.reuse_controller_sample_jacobian
            && control_guard.is_some_and(|g| self.control.permits_jacobian_reuse_after_sample(g));
        if !keep_proposal {
            state.workspace.clear();
        }
        if guard < self.bank.guard_count() {
            let boundaries =
                self.control
                    .boundaries(t, &state.mechanics, &state.auxiliary, &state.held)?;
            self.bank.jump(
                guard,
                t,
                &state.mechanics,
                &mut state.auxiliary,
                &boundaries,
            )
        } else {
            self.control.jump(
                guard - self.bank.guard_count(),
                t,
                &state.mechanics,
                &mut state.held,
            )
        }
    }
}

impl EmbeddedMotorBank {
    /// Advance a fixed-boundary motor/mechanism interval with shared event
    /// scheduling. Startup classifications and engagement/release jumps use the
    /// registered motor implementation. All states are local until success;
    /// unresolved roots or event limits fail instead of silently skipping jumps.
    /// Supply pure external loads, hold drive boundaries, and split intervals
    /// at any external controller deadlines. This is not a thermal/firmware
    /// adapter or a local-truncation-error controller.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_with_events<F>(
        &mut self,
        map: &RigidEmbedding<'_>,
        seed: &Generalized,
        motor_seed: &[f64],
        time_s: f64,
        step_s: f64,
        implicit: &ImplicitStepConfig,
        hybrid: &HybridConfig,
        boundaries: &[MotorBoundary],
        external: F,
    ) -> Result<EmbeddedMotorAdvance, String>
    where
        F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
    {
        self.advance_with_boundary_law(
            map,
            seed,
            motor_seed,
            time_s,
            step_s,
            implicit,
            hybrid,
            |_, _, _| Ok(boundaries.to_vec()),
            external,
        )
    }

    /// Boundary laws are evaluated afresh at every trial current/mechanical
    /// state, including event searches. They MUST be pure; controller clocks,
    /// battery and thermal states are not silently advanced here. Split the
    /// interval at external command/sampling deadlines. Failure is atomic.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_with_boundary_law<F, B>(
        &mut self,
        map: &RigidEmbedding<'_>,
        seed: &Generalized,
        motor_seed: &[f64],
        time_s: f64,
        step_s: f64,
        implicit: &ImplicitStepConfig,
        hybrid: &HybridConfig,
        boundary_law: B,
        external: F,
    ) -> Result<EmbeddedMotorAdvance, String>
    where
        F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
        B: Fn(f64, &Generalized, &[f64]) -> Result<Vec<MotorBoundary>, String>,
    {
        let mut control = ContinuousBoundaries(boundary_law);
        self.advance_with_control(
            map,
            seed,
            motor_seed,
            &(),
            time_s,
            step_s,
            implicit,
            hybrid,
            &mut control,
            external,
        )
        .map(|r| r.motor)
    }

    /// Advance held-state controls and motor events in one atomic scheduler.
    /// Control guards follow motor guards, identified by control_guard_offset.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_with_control<F, C>(
        &mut self,
        map: &RigidEmbedding<'_>,
        seed: &Generalized,
        motor_seed: &[f64],
        control_seed: &C::State,
        time_s: f64,
        step_s: f64,
        implicit: &ImplicitStepConfig,
        hybrid: &HybridConfig,
        control: &mut C,
        external: F,
    ) -> Result<EmbeddedControlledAdvance<C::State>, String>
    where
        F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
        C: SampledMotorControl,
    {
        self.advance_with_control_cached(
            map,
            seed,
            motor_seed,
            control_seed,
            time_s,
            step_s,
            implicit,
            hybrid,
            control,
            &mut ImplicitSolverWorkspace::default(),
            external,
        )
    }

    /// As advance_with_control, with an opt-in numerical workspace carried
    /// between intervals. All workspace changes commit only after the complete
    /// interval and endpoint validation succeed. Every discrete jump clears it.
    /// Caller must clear after model edits, state resets or new external laws.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_with_control_cached<F, C>(
        &mut self,
        map: &RigidEmbedding<'_>,
        seed: &Generalized,
        motor_seed: &[f64],
        control_seed: &C::State,
        time_s: f64,
        step_s: f64,
        implicit: &ImplicitStepConfig,
        hybrid: &HybridConfig,
        control: &mut C,
        workspace: &mut ImplicitSolverWorkspace,
        external: F,
    ) -> Result<EmbeddedControlledAdvance<C::State>, String>
    where
        F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
        C: SampledMotorControl,
    {
        let initial = MotorState {
            held: control_seed.clone(),
            workspace: workspace.clone(),
            mechanics: seed.clone(),
            auxiliary: motor_seed.to_vec(),
            contacts: None,
        };
        let (result, solves) = {
            let mut stepper = MotorStepper {
                bank: self,
                map,
                config: implicit,
                control,
                external: &external,
                stats: RefCell::new(MotorSolveStatistics::default()),
            };
            let result = advance_interval(&mut stepper, &initial, time_s, step_s, hybrid)?;
            (result, stepper.stats.into_inner())
        };
        let state = result.state;
        let contact_steps = self.audit_contact_steps.then(|| {
            let mut steps = Vec::new();
            let mut node = state.contacts.as_ref();
            while let Some(at) = node {
                steps.push(at.step.clone());
                node = at.previous.as_ref();
            }
            steps.reverse();
            steps
        });
        if contact_steps
            .as_ref()
            .is_some_and(|steps| steps.len() != result.diagnostics.accepted_segments)
        {
            return Err("incomplete accepted motor contact trace".into());
        }
        let positions: Vec<_> = map
            .independent
            .iter()
            .map(|i| state.mechanics.q[*i])
            .collect();
        let motion = map.solve(
            &state.mechanics,
            &positions,
            &map.reduced_velocity(&state.mechanics),
        )?;
        let mut loads = self
            .evaluate(
                result.time_s,
                step_s,
                &motion.generalized,
                &state.auxiliary,
                &state.auxiliary,
                &control.boundaries(
                    result.time_s,
                    &motion.generalized,
                    &state.auxiliary,
                    &state.held,
                )?,
            )?
            .0
            .generalized_loads;
        let extra = external(result.time_s, &motion.generalized)?;
        if extra.len() != loads.len() || extra.iter().any(|v| !v.is_finite()) {
            return Err("invalid endpoint external loads".into());
        }
        for (f, e) in loads.iter_mut().zip(extra) {
            *f += e;
        }
        let endpoint = map.accelerations(&motion, &loads)?;
        *workspace = state.workspace;
        Ok(EmbeddedControlledAdvance {
            control_state: state.held,
            control_guard_offset: self.guard_count(),
            motor: EmbeddedMotorAdvance {
                time_s: result.time_s,
                endpoint,
                motor_states: state.auxiliary,
                hybrid: result.diagnostics,
                solves,
                contact_steps,
            },
        })
    }
}

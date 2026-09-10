//! Bounded failure recovery for pure mechanical force laws. All integration and
//! retry scheduling comes from the existing implicit and hybrid implementations.
use super::{
    CoupledForces, EmbeddedAcceleration, Generalized, ImplicitSolverWorkspace, ImplicitStepConfig,
    ImplicitStepDiagnostics, RigidEmbedding,
};
use sim_dynamics::hybrid::{HybridConfig, HybridDiagnostics, HybridStepper, advance_interval};
use std::rc::Rc;

#[derive(Clone, Debug, serde::Serialize)]
pub struct MechanicalSegment {
    pub start_time_s: f64,
    pub step_s: f64,
    pub diagnostics: ImplicitStepDiagnostics,
    /// Additional internal solve of an SDIRK2 macrostep, not a physical substep.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_stage_diagnostics: Option<ImplicitStepDiagnostics>,
}
pub struct EmbeddedMechanicalAdvance {
    pub endpoint: EmbeddedAcceleration,
    pub segments: Vec<MechanicalSegment>,
    pub refinement: HybridDiagnostics,
}
#[derive(Clone)]
struct State {
    mechanics: Generalized,
    workspace: ImplicitSolverWorkspace,
    endpoint: Option<Rc<EmbeddedAcceleration>>,
    segments: Vec<MechanicalSegment>,
}
struct Stepper<'a, 'm, F> {
    map: &'a RigidEmbedding<'m>,
    config: &'a ImplicitStepConfig,
    loads: F,
}
impl<F> HybridStepper for Stepper<'_, '_, F>
where
    F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
{
    type State = State;
    fn advance(&self, t: f64, h: f64, state: &State) -> Result<State, String> {
        let mut workspace = state.workspace.clone();
        if self.config.sdirk2 {
            let (endpoint,first,second)=self.map.sdirk_mechanical_step(&state.mechanics,t,h,self.config,&self.loads)?;
            workspace.clear();
            let mut segments=state.segments.clone();
            segments.push(MechanicalSegment{start_time_s:t,step_s:h,diagnostics:second,
                first_stage_diagnostics:Some(first)});
            return Ok(State{mechanics:endpoint.generalized.clone(),workspace,
                endpoint:Some(Rc::new(endpoint)),segments});
        }
        let solve = |workspace: &mut ImplicitSolverWorkspace, config: &ImplicitStepConfig| {
            self.map.step_implicit_coupled_cached(
                &state.mechanics,
                &[],
                t,
                h,
                config,
                workspace,
                None,
                true,
                |t, _, g, _, _| {
                    Ok(CoupledForces {
                        generalized_loads: (self.loads)(t, g)?,
                        auxiliary_residuals: vec![],
                    })
                },
            )
        };
        let cached = self.config.reuse_step_jacobian && state.workspace.has_jacobian();
        let mut first_config = self.config.clone();
        if cached {
            if let Some(limit) = self.config.cached_mechanical_iteration_limit {
                first_config.newton.max_iterations = limit;
            }
        }
        let (mut step, fresh_restart_reason) = match solve(&mut workspace, &first_config) {
            Ok(step) => (step, None),
            Err(first) if self.config.restart_failed_reused_mechanics && cached => {
                workspace.clear();
                let step = solve(&mut workspace, self.config).map_err(|fresh| {
                    format!("cached trial failed: {first}; fresh restart failed: {fresh}")
                })?;
                (step, Some(first))
            }
            Err(error) => return Err(error),
        };
        step.diagnostics.fresh_restart_reason = fresh_restart_reason;
        let mut segments = state.segments.clone();
        segments.push(MechanicalSegment {
            start_time_s: t,
            step_s: h,
            diagnostics: step.diagnostics,
            first_stage_diagnostics: None,
        });
        Ok(State {
            mechanics: step.endpoint.generalized.clone(),
            workspace,
            endpoint: Some(Rc::new(step.endpoint)),
            segments,
        })
    }
    fn guards(&self, t: f64, state: &State) -> Result<Vec<f64>, String> {
        Ok(self.map.art.imus.iter().map(|imu| state.mechanics.states[imu.state + 15] - t).collect())
    }
    fn scheduled(&self, _: f64, state: &State) -> Result<Vec<(usize, f64)>, String> {
        Ok(self.map.art.imus.iter().enumerate().map(|(i, imu)| (i, state.mechanics.states[imu.state + 15])).collect())
    }
    fn jump(&mut self, index: usize, _: f64, state: &mut State) -> Result<(), String> {
        self.map.art.sample_imu_event(index, &mut state.mechanics)
    }
}
impl RigidEmbedding<'_> {
    // One implementation for all adapters; avoid duplicating both large nested
    // implicit solvers for each caller's load-closure type.
    #[allow(clippy::type_complexity)]
    fn sdirk_mechanical_step(&self,seed:&Generalized,t:f64,h:f64,config:&ImplicitStepConfig,
        loads:&dyn Fn(f64,&Generalized)->Result<Vec<f64>,String>)
        ->Result<(EmbeddedAcceleration,ImplicitStepDiagnostics,ImplicitStepDiagnostics),String>
    {
        let mut config=config.clone();config.sdirk2=false;
        let gamma=sim_dynamics::sdirk::GAMMA;
        // Each stage has a fresh workspace. No accepted-state history is
        // extrapolated across a held-controller boundary.
        let first=self.step_implicit(seed,t,gamma*h,&config,|_,g|loads(t+gamma*h,g))?;
        let(trial,q,u)=self.trial_state(seed,(1.-gamma)*h,&first.endpoint.generalized,&first.endpoint)?;
        // This closed chart point is an affine stage anchor, not a physical
        // intermediate state. Floating rotations use composed world-frame
        // exponentials; contact history uses the same RK weights.
        let anchor=self.solve(&trial,&q,&u)?;
        // Match the shared vector SDIRK primitive: start Newton from the first
        // physical stage, not the extrapolated affine anchor. The equation
        // still uses that anchor and all original endpoint checks.
        let first_velocity=self.reduced_velocity(&first.endpoint.generalized);
        let second=self.step_implicit_with_velocity_guess(&anchor.generalized,t+(1.-gamma)*h,gamma*h,&config,
            &first_velocity,|_,g|loads(t+h,g))?;
        Ok((second.endpoint,first.diagnostics,second.diagnostics))
    }
    /// Retry failed backward-Euler trials using the shared bounded subdivision
    /// scheduler. Every accepted segment uses unchanged residual/closure checks.
    /// The whole interval is atomic: failure never changes the supplied seed.
    ///
    /// Loads must be pure. Hold sampled controller commands and split at known
    /// input deadlines outside this call. Auxiliary actuator states and discrete
    /// events require their existing coupled/event adapters. Subdivision recovers
    /// convergence failures; it is not local-error estimation or impact location.
    pub fn advance_implicit_mechanics<F>(
        &self,
        seed: &Generalized,
        time_s: f64,
        step_s: f64,
        config: &ImplicitStepConfig,
        refinement: &HybridConfig,
        loads: F,
    ) -> Result<EmbeddedMechanicalAdvance, String>
    where
        F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
    {
        self.advance_implicit_mechanics_cached(
            seed,
            time_s,
            step_s,
            config,
            refinement,
            &mut ImplicitSolverWorkspace::default(),
            loads,
        )
    }

    /// Bounded mechanical recovery with caller-owned derivative reuse. Only
    /// successful complete intervals commit the workspace. Rejected trials and
    /// failed intervals cannot contaminate it. Existing solver guards invalidate
    /// reuse after timestep/layout/contact changes or poor convergence.
    ///
    /// Clear the workspace after edits, resets, or changed force-law definitions.
    /// Sampled controllers must explicitly opt into reuse across command updates.
    /// Physical states and force evaluations are always fresh.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_implicit_mechanics_cached<F>(
        &self,
        seed: &Generalized,
        time_s: f64,
        step_s: f64,
        config: &ImplicitStepConfig,
        refinement: &HybridConfig,
        workspace: &mut ImplicitSolverWorkspace,
        loads: F,
    ) -> Result<EmbeddedMechanicalAdvance, String>
    where
        F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
    {
        if config.restart_failed_reused_mechanics && !config.reuse_step_jacobian {
            return Err("fresh mechanical restart requires cross-step Jacobian reuse".into());
        }
        if config.cached_mechanical_iteration_limit.is_some_and(|n| {
            n == 0 || n > config.newton.max_iterations || !config.restart_failed_reused_mechanics
        }) {
            return Err("cached mechanical iteration limit requires fresh restart and must be within the original Newton limit".into());
        }
        let mut stepper = Stepper {
            map: self,
            config,
            loads,
        };
        let initial = State {
            mechanics: seed.clone(),
            workspace: workspace.clone(),
            endpoint: None,
            segments: vec![],
        };
        let result = advance_interval(&mut stepper, &initial, time_s, step_s, refinement)?;
        let mut endpoint = Rc::try_unwrap(
            result
                .state
                .endpoint
                .ok_or("mechanical interval produced no endpoint")?,
        )
        .map_err(|_| "mechanical interval retained a temporary endpoint")?;
        endpoint.generalized = result.state.mechanics;
        *workspace = result.state.workspace;
        Ok(EmbeddedMechanicalAdvance {
            endpoint,
            segments: result.state.segments,
            refinement: result.diagnostics,
        })
    }
}

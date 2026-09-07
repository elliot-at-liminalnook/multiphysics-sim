//! Bounded failure recovery for pure mechanical force laws. All integration and
//! retry scheduling comes from the existing implicit and hybrid implementations.
use super::{
    EmbeddedAcceleration, Generalized, ImplicitStepConfig, ImplicitStepDiagnostics, RigidEmbedding,
};
use sim_dynamics::hybrid::{HybridConfig, HybridDiagnostics, HybridStepper, advance_interval};
use std::rc::Rc;

#[derive(Clone, Debug, serde::Serialize)]
pub struct MechanicalSegment {
    pub start_time_s: f64,
    pub step_s: f64,
    pub diagnostics: ImplicitStepDiagnostics,
}
pub struct EmbeddedMechanicalAdvance {
    pub endpoint: EmbeddedAcceleration,
    pub segments: Vec<MechanicalSegment>,
    pub refinement: HybridDiagnostics,
}
#[derive(Clone)]
struct State {
    mechanics: Generalized,
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
        let step = self
            .map
            .step_implicit(&state.mechanics, t, h, self.config, &self.loads)?;
        let mut segments = state.segments.clone();
        segments.push(MechanicalSegment {
            start_time_s: t,
            step_s: h,
            diagnostics: step.diagnostics,
        });
        Ok(State {
            mechanics: step.endpoint.generalized.clone(),
            endpoint: Some(Rc::new(step.endpoint)),
            segments,
        })
    }
    fn guards(&self, _: f64, _: &State) -> Result<Vec<f64>, String> {
        Ok(vec![])
    }
    fn jump(&mut self, _: usize, _: f64, _: &mut State) -> Result<(), String> {
        Err("pure mechanical advancement has no scheduled jumps".into())
    }
}
impl RigidEmbedding<'_> {
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
        let mut stepper = Stepper {
            map: self,
            config,
            loads,
        };
        let initial = State {
            mechanics: seed.clone(),
            endpoint: None,
            segments: vec![],
        };
        let result = advance_interval(&mut stepper, &initial, time_s, step_s, refinement)?;
        let endpoint = Rc::try_unwrap(
            result
                .state
                .endpoint
                .ok_or("mechanical interval produced no endpoint")?,
        )
        .map_err(|_| "mechanical interval retained a temporary endpoint")?;
        Ok(EmbeddedMechanicalAdvance {
            endpoint,
            segments: result.state.segments,
            refinement: result.diagnostics,
        })
    }
}

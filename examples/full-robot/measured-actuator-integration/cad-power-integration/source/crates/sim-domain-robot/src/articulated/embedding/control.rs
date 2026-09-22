//! Held-state motor boundary controls. Continuous electrical/mechanical states
//! stay in the coupled solve, including explicitly supplied power/thermal states.
use super::{Generalized, MotorBoundary};

pub trait SampledMotorControl {
    type State: Clone;
    /// Check the admissible model envelope at candidate segment endpoints and
    /// jumps. Failure rejects the candidate interval; this is not a BMS model.
    fn validate_endpoint(&self, _held: &Self::State) -> Result<(), String> { Ok(()) }
    /// Continuous states carried alongside held controls in checkpoints. The
    /// scheduler includes these unknowns in the same solve as motor/mechanics
    /// states; it never advances them using a previous accepted current.
    fn continuous_states(&self, _held: &Self::State) -> Vec<f64> {
        vec![]
    }
    fn set_continuous_states(&self, _held: &mut Self::State, states: &[f64]) -> Result<(), String> {
        if states.is_empty() { Ok(()) } else { Err("unexpected continuous control states".into()) }
    }
    /// Restore algebraic consistency at startup and after discrete control
    /// changes. Differential states (charge, energy, temperature) must remain
    /// unchanged. Mutation is confined to the scheduler's candidate state.
    fn reconcile_continuous_states(&self, _time: f64, _mechanics: &Generalized,
        _motors: &[f64], _held: &mut Self::State) -> Result<(), String> { Ok(()) }
    /// Residuals in continuous_states order, at the current trial point. The
    /// caller has installed trial continuous states into held before this call.
    fn continuous_residuals(
        &self,
        _time: f64,
        _mechanics: &Generalized,
        _motors: &[f64],
        _held: &Self::State,
        rates: &[f64],
    ) -> Result<Vec<f64>, String> {
        if rates.is_empty() { Ok(vec![]) } else { Err("missing continuous control equations".into()) }
    }
    /// Pure boundary evaluation. No sampling, random draws, or accepted-state
    /// mutation in a Newton or event-location trial.
    fn boundaries(
        &self,
        time: f64,
        mechanics: &Generalized,
        motors: &[f64],
        held: &Self::State,
    ) -> Result<Vec<MotorBoundary>, String>;
    /// At fixed mechanics/time/held control, boundary i depends only on motor
    /// i's internal states, in the bank's exact named order. This must hold for
    /// all trial states and modes. Shared current-dependent supplies or thermal
    /// boundaries generally violate it. Unknown adapters deliberately opt out.
    fn independent_motor_boundaries(&self) -> bool {
        false
    }
    fn guards(
        &self,
        _time: f64,
        _mechanics: &Generalized,
        _held: &Self::State,
    ) -> Result<Vec<f64>, String> {
        Ok(vec![])
    }
    fn scheduled(
        &self,
        _time: f64,
        _mechanics: &Generalized,
        _held: &Self::State,
    ) -> Result<Vec<(usize, f64)>, String> {
        Ok(vec![])
    }
    /// True only for known held-control sampling events that retain the
    /// continuous equation structure. Values/derivatives may change: a retained
    /// matrix is merely a modified-Newton proposal, subject to fresh residuals,
    /// contraction checks and fallback refresh. Unknown adapters default false.
    fn permits_jacobian_reuse_after_sample(&self, _guard: usize) -> bool {
        false
    }
    /// Mutate only supplied state: a later failure discards the whole interval.
    fn jump(
        &mut self,
        guard: usize,
        time: f64,
        mechanics: &Generalized,
        held: &mut Self::State,
    ) -> Result<(), String>;
}

pub(super) struct ContinuousBoundaries<B>(pub B);
impl<B> SampledMotorControl for ContinuousBoundaries<B>
where
    B: Fn(f64, &Generalized, &[f64]) -> Result<Vec<MotorBoundary>, String>,
{
    type State = ();
    fn boundaries(
        &self,
        time: f64,
        g: &Generalized,
        motors: &[f64],
        _: &(),
    ) -> Result<Vec<MotorBoundary>, String> {
        (self.0)(time, g, motors)
    }
    fn jump(&mut self, _: usize, _: f64, _: &Generalized, _: &mut ()) -> Result<(), String> {
        Err("continuous boundary has no jump".into())
    }
}

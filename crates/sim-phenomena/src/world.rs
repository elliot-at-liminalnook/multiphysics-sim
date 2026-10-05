//! The registry every surprise compiles against, and small authoring helpers.

use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, BlockInterface, BlockPort, BlockTiming, ImplementationRef, Instance, ModelWorld, QuantityKind, StateId};
use sim_dynamics::{Integrator, Trace};
use sim_solve::NewtonConfig;

pub use sim_runtime::registry;

/// Compile a model with the implicit midpoint rule and a patient Newton:
/// compiled islands carry stiff constitutive kinks (regularised friction,
/// dead zones) that a dense hand-written residual never had to survive.
pub fn runtime(model: ModelWorld, registry: &BehaviorRegistry) -> Runtime {
    Runtime::new(model, registry, Integrator::ImplicitMidpoint(newton())).expect("model compiles")
}

/// The suite's Newton settings.
pub fn newton() -> NewtonConfig {
    NewtonConfig { max_iterations: 40, min_line_search: 1.0 / 4096.0, ..NewtonConfig::default() }
}

/// A runtime on the L-stable backward Euler rule, for stiff networks whose
/// fast modes are to be damped rather than followed.
pub fn damped_runtime(model: ModelWorld, registry: &BehaviorRegistry) -> Runtime {
    Runtime::new(model, registry, Integrator::BackwardEuler(newton())).expect("model compiles")
}

/// Run and record; panics with the runtime error on failure so scenario
/// code stays linear.
pub fn record(runtime: &mut Runtime, duration: f64, h: f64, every: usize, ids: &[StateId]) -> Trace {
    runtime.advance_recording(duration, h, every, ids).expect("simulation runs")
}

/// A host controller block named `name` (a lockstep coupler answering at
/// each tick of `period`) with typed inputs and outputs, each list sorted by
/// name: the order its contract lists them. Outputs start at 0.
pub fn controller_block(m: &mut ModelWorld, name: &str, period: f64, mut inputs: Vec<(String, QuantityKind)>, mut outputs: Vec<(String, QuantityKind)>) -> Instance {
    inputs.sort_by(|a, b| a.0.cmp(&b.0));
    outputs.sort_by(|a, b| a.0.cmp(&b.0));
    let interface = BlockInterface {
        inputs: inputs.into_iter().map(|(n, k)| BlockPort::new(n, k)).collect(),
        outputs: outputs.into_iter().map(|(n, k)| BlockPort::new(n, k).start(0.0)).collect(),
        feedthrough: true,
    };
    m.add_block(name, interface, BlockTiming::periodic(period), ImplementationRef::Host { name: name.into() }).expect("a valid controller block")
}

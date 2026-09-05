//! The registry every surprise compiles against, and small authoring helpers.

use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_dynamics::{Integrator, Trace};
use sim_solve::NewtonConfig;

/// Every domain's compiled elements in one registry.
pub fn registry() -> BehaviorRegistry {
    let mut registry = BehaviorRegistry::default();
    sim_domain_rotational::elements::register(&mut registry).unwrap();
    sim_domain_translational::elements::register(&mut registry).unwrap();
    sim_domain_electrical::elements::register(&mut registry).unwrap();
    sim_domain_thermal::register(&mut registry).unwrap();
    sim_domain_hydraulic::register(&mut registry).unwrap();
    sim_domain_acoustic::register(&mut registry).unwrap();
    sim_domain_fluid::register(&mut registry).unwrap();
    sim_domain_fluid::twophase::register(&mut registry).unwrap();
    sim_domain_control::elements::register(&mut registry).unwrap();
    sim_domain_bridges::elements::register(&mut registry).unwrap();
    sim_domain_multibody::elements::register(&mut registry).unwrap();
    sim_domain_multibody::planar::register(&mut registry).unwrap();
    sim_domain_multibody::contact::register(&mut registry).unwrap();
    sim_domain_multibody::chain::register(&mut registry).unwrap();
    sim_domain_magnetic::register(&mut registry).unwrap();
    sim_domain_chemical::register(&mut registry).unwrap();
    sim_domain_radiative::register(&mut registry).unwrap();
    sim_domain_line::register(&mut registry).unwrap();
    sim_domain_granular::register(&mut registry).unwrap();
    sim_domain_sensing::register(&mut registry).unwrap();
    sim_domain_robot::register(&mut registry).unwrap();
    registry
}

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

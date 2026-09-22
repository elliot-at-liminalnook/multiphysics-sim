//! Shared CAD-derived multiphysics runtime for native, browser and headless hosts.

use sim_core::BehaviorRegistry;

pub mod acquisition;
pub mod contact_audit;
pub mod contact_planning;
pub mod contact_reference;
pub mod contact_exploration;
pub mod motion_evaluation;
pub mod experiment_variants;
pub mod geometry_evaluation;
pub mod numerical_validation;
#[cfg(all(feature = "evolution", not(target_arch = "wasm32")))]
pub mod search_comparison;
pub mod contact_implicit;
pub mod configuration_inspection;
pub mod system_inspection;
pub mod system_session;
#[cfg(not(target_arch = "wasm32"))]
pub mod system_worker;
pub mod robot_contract;
pub mod robot_input;
pub mod body_feedback;
pub mod embedded;
pub mod embedded_policy;
pub mod environment;
pub mod policy_evaluation;
pub mod predictive_control;
pub mod lift;
pub mod physical;
pub mod planning;
pub mod motion_tracking;
pub mod posture;
pub mod session;
pub mod support;
pub mod tracking;
pub mod validation;
pub use physical::{BuildOptions, PhysicalRobot};

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
    sim_domain_control::pulse::register(&mut registry).unwrap();
    sim_domain_bridges::elements::register(&mut registry).unwrap();
    sim_domain_multibody::elements::register(&mut registry).unwrap();
    sim_domain_multibody::planar::register(&mut registry).unwrap();
    sim_domain_multibody::contact::register(&mut registry).unwrap();
    sim_domain_multibody::smooth_contact::register(&mut registry).unwrap();
    sim_domain_multibody::chain::register(&mut registry).unwrap();
    sim_domain_magnetic::register(&mut registry).unwrap();
    sim_domain_chemical::register(&mut registry).unwrap();
    sim_domain_radiative::register(&mut registry).unwrap();
    sim_domain_line::register(&mut registry).unwrap();
    sim_domain_granular::register(&mut registry).unwrap();
    sim_domain_sensing::register(&mut registry).unwrap();
    sim_domain_robot::register(&mut registry).unwrap();
    evaluation_primitives::register(&mut registry).unwrap();
    exploration::register(&mut registry).unwrap();
    contact_exploration::register(&mut registry).unwrap();
    experiment_variants::register(&mut registry).unwrap();
    geometry_evaluation::register(&mut registry).unwrap();
    numerical_validation::register(&mut registry).unwrap();
    registry
}


/// Robust default for coupled stiff constitutive equations.
pub fn newton() -> sim_solve::NewtonConfig {
    sim_solve::NewtonConfig { max_iterations: 40, min_line_search: 1.0 / 4096.0, ..Default::default() }
}

pub mod task_observation;
pub mod imu_observation;

pub mod point_feedback;
pub mod step_reference;
pub mod walking_task;
pub mod speed_task;
pub mod progress_task;
pub mod ppo_training;
pub mod motion_data;
pub mod motion_forecast;
pub mod motion_parameters;
pub mod motion_response;
pub mod experiment;
pub mod exploration;
pub mod evaluation_primitives;
pub mod experiment_comparison;
pub mod experiment_study;
pub mod actuator_bench;
pub mod controller_refinement;
#[cfg(all(feature = "bayesian", not(target_arch = "wasm32")))]
pub mod experiment_search;
pub mod forecast_actions;
pub mod fidelity;
pub mod physics_context;
pub mod predictive_policy;

pub mod electrical;

//! Shared CAD-derived multiphysics runtime for native, browser and headless hosts.

use sim_core::BehaviorRegistry;

pub mod contact_audit;
pub mod body_feedback;
pub mod embedded;
pub mod embedded_policy;
pub mod environment;
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


/// Robust default for coupled stiff constitutive equations.
pub fn newton() -> sim_solve::NewtonConfig {
    sim_solve::NewtonConfig { max_iterations: 40, min_line_search: 1.0 / 4096.0, ..Default::default() }
}

pub mod task_observation;

pub mod point_feedback;
pub mod step_reference;
pub mod walking_task;

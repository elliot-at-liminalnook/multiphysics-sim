//! Shared CAD-derived multiphysics runtime for native, browser and headless hosts.

use sim_core::BehaviorRegistry;

pub mod acquisition;
pub mod actuator_registry;
pub mod gait_playback;
pub mod contact_audit;
pub mod contact_planning;
pub mod contact_reference;
pub mod contact_exploration;
pub mod gait_lab;
pub mod motion_evaluation;
pub mod experiment_variants;
pub mod geometry_evaluation;
pub mod numerical_validation;
#[cfg(all(feature = "evolution", not(target_arch = "wasm32")))]
pub mod search_comparison;
pub mod contact_implicit;
pub mod configuration_inspection;
pub mod kinematic_mirror;
pub mod system_inspection;
pub mod system_session;
pub mod system_launch;
pub mod system_builder;
pub mod system_study;
pub mod bench;
pub mod run_history;
pub mod lesson;
pub mod lesson_draft;
pub mod lesson_lab;
pub mod lesson_model;
pub mod realtime_fidelity;
pub mod part_fit;
#[cfg(not(target_arch = "wasm32"))]
pub mod system_worker;
pub mod robot_contract;
pub mod robot_input;
pub mod body_feedback;
pub mod embedded;
pub mod embedded_capture;
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
pub mod workspace;
pub use physical::{BuildOptions, PhysicalRobot};
/// Re-exported so browser and other hosts reach the system-builder stack
/// through this one crate.
pub use sim_inspect as inspect;
pub use sim_parts as parts;
pub use sim_system as system;

/// Every domain's compiled elements in one registry.
/// The built-in registry plus every authored part (`*.part`) in `dir`.
/// Parts that fail to load are reported, not silently skipped.
pub fn registry_with_parts(dir: &std::path::Path) -> (BehaviorRegistry, Vec<sim_parts::Loaded>) {
    let mut registry = registry();
    let loaded = sim_parts::load_dir(&mut registry, dir);
    (registry, loaded)
}

/// The registry every system-document tool uses: built-ins plus authored
/// parts from `$SIM_PARTS_DIR`, else `<workspace root>/library/parts` with the
/// process's root ([`workspace::get`]: resolved at launch, else from
/// `$SIM_WORKSPACE` and the current directory). Load errors are printed with
/// file and line; the rest still load. With no root, a warning names the
/// directories searched and only the built-ins load.
pub fn system_registry() -> BehaviorRegistry {
    system_registry_in(workspace::get().as_ref())
}

/// [`system_registry`] against a given workspace resolution.
pub fn system_registry_in(root: Result<&workspace::WorkspaceRoot, &workspace::WorkspaceError>) -> BehaviorRegistry {
    let dir = match (std::env::var_os("SIM_PARTS_DIR"), root) {
        (Some(dir), _) => std::path::PathBuf::from(dir),
        (None, Ok(root)) => root.path.join("library/parts"),
        (None, Err(e)) => {
            eprintln!("authored parts not loaded (built-ins only): {e}");
            return registry();
        }
    };
    let (registry, loaded) = registry_with_parts(&dir);
    for l in loaded.iter().filter(|l| l.error.is_some()) {
        eprintln!("part not loaded: {}", l.error.as_deref().unwrap_or_default());
    }
    registry
}

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
    sim_domain_control::pwm::register(&mut registry).unwrap();
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
    // Learning notes for components registered above (each crate keeps its own).
    sim_domain_electrical::notes::annotate(&mut registry);
    sim_domain_thermal::notes::annotate(&mut registry);
    sim_domain_translational::notes::annotate(&mut registry);
    sim_domain_sensing::notes::annotate(&mut registry);
    sim_domain_robot::notes::annotate(&mut registry);
    sim_domain_control::notes::annotate(&mut registry);
    sim_domain_bridges::notes::annotate(&mut registry);
    sim_domain_magnetic::notes::annotate(&mut registry);
    sim_domain_multibody::notes::annotate(&mut registry);
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
pub(crate) mod online_reference;
pub mod steered_reference;
pub mod system_display;
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

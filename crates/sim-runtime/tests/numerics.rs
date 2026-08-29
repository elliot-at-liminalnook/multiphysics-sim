use glam::Vec3;
use sim_geometry::{RestTransform, VisualBinding, VisualMotion, project};
use sim_runtime::{ActuatorConfig, ActuatorSimulation};

fn endpoint(step: f64) -> f64 {
    let config = ActuatorConfig {
        plant_step: step,
        trace_every_steps: u64::MAX,
        ..ActuatorConfig::default()
    };
    let mut simulation = ActuatorSimulation::new(config).unwrap();
    simulation.inputs.target_position = 0.005;
    simulation.run_for(0.25).unwrap();
    simulation.sample().unwrap().position
}

#[test]
fn midpoint_solution_converges_as_step_is_halved() {
    let reference = endpoint(12.5e-6);
    let error_100 = (endpoint(100.0e-6) - reference).abs();
    let error_50 = (endpoint(50.0e-6) - reference).abs();
    let error_25 = (endpoint(25.0e-6) - reference).abs();
    assert!(error_50 < error_100, "{error_50:e} !< {error_100:e}");
    assert!(error_25 < error_50, "{error_25:e} !< {error_50:e}");
}

#[test]
fn one_second_contains_exactly_one_thousand_controller_samples() {
    let mut simulation = ActuatorSimulation::new(ActuatorConfig::default()).unwrap();
    let summary = simulation.run_for(1.0).unwrap();
    assert_eq!(summary.steps, 20_000);
    assert_eq!(summary.controller_samples, 1_000);
}

#[test]
fn visual_projection_reads_the_asserted_carriage_state_id() {
    let mut simulation = ActuatorSimulation::new(ActuatorConfig::default()).unwrap();
    simulation.run_for(0.2).unwrap();
    let object = simulation
        .model
        .objects
        .iter()
        .find_map(|(id, object)| (object.name == "linear carriage").then_some(id))
        .unwrap();
    let binding = VisualBinding {
        object,
        source: simulation.ids.carriage_position,
        motion: VisualMotion::Translate { axis: Vec3::X },
        scale: 8.0,
    };
    let projected = project(binding, RestTransform::default(), &simulation.model.state).unwrap();
    let asserted = simulation.sample().unwrap().position as f32 * 8.0;
    assert!((projected.translation.x - asserted).abs() < 1.0e-6);
}

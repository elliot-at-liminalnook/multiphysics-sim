use serde_json::{Value, json};
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession},
    session::{Scene, Session},
};

fn fixture() -> (Scene, Config, usize) {
    let mut robot: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/baseline/robot.simrobot.json"
    ))
    .unwrap();
    // Isolate mechanical-coordinate support. The intended robot retains its IMU;
    // authored IMU scheduling is a separate pending acceptance requirement.
    robot["sensors"]
        .as_array_mut()
        .unwrap()
        .retain(|s| s["kind"] != "imu");
    let scene: Scene = serde_json::from_value(json!({"version":1,"robot":robot,
        "options":{"contact":false,"flex":false},"period_s":0.02,"duration_s":0.02}))
    .unwrap();
    let raw = Session::new(scene.clone(), 0).unwrap();
    let passive = raw
        .robot
        .art
        .dofs()
        .position(|(_, d)| d.name == "joint.passive axle")
        .unwrap();
    let mut loads = vec![0.; 6 + raw.robot.art.dofs().count()];
    loads[6 + passive] = 0.001;
    let config = serde_json::from_value(json!({"step_s":0.00025,"steps":20,"report_every":1,
        "independent_coordinates":["joint.passive axle","joint.right axle","joint.left axle"],
        "applied_generalized_loads":loads,"implicit":{}}))
    .unwrap();
    (scene, config, passive)
}

#[test]
fn passive_coordinate_advances_and_replays_without_an_actuator() {
    let (scene, config, passive) = fixture();
    let original = serde_json::to_value(&scene.robot).unwrap();
    let mut runner =
        EmbeddedSession::new(scene.clone(), config.clone(), 91, CaptureMode::Latest).unwrap();
    assert_eq!(
        runner.coordinate_names(),
        config.independent_coordinates.as_ref().unwrap()
    );
    runner.advance(20).unwrap();
    let frame = runner.frame().unwrap();
    assert!(frame["joint_velocities"][passive].as_f64().unwrap() > 0.);
    assert_eq!(
        serde_json::to_value(&runner.recording().scene.robot).unwrap(),
        original
    );
    assert_eq!(runner.recording().scene.robot.motors.len(), 2);
    let (mut replay, steps) =
        EmbeddedSession::prepare_replay(runner.recording(), CaptureMode::Latest).unwrap();
    replay.advance(steps).unwrap();
    assert_eq!(frame, replay.frame().unwrap());

    let mut reordered = config.clone();
    reordered
        .independent_coordinates
        .as_mut()
        .unwrap()
        .reverse();
    let mut other =
        EmbeddedSession::new(scene.clone(), reordered, 91, CaptureMode::Latest).unwrap();
    other.advance(20).unwrap();
    for key in ["joint_positions", "joint_velocities"] {
        for (a, b) in frame[key]
            .as_array()
            .unwrap()
            .iter()
            .zip(other.frame().unwrap()[key].as_array().unwrap())
        {
            assert!(
                (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-10,
                "{key}"
            );
        }
    }
    let mut legacy = config;
    legacy.independent_coordinates = None;
    let mut underconstrained =
        EmbeddedSession::new(scene, legacy, 91, CaptureMode::Latest).unwrap();
    assert!(
        underconstrained
            .advance(1)
            .unwrap_err()
            .contains("underconstrained")
    );
}

#[test]
fn rejects_duplicate_missing_and_underconstrained_coordinate_selections() {
    let (scene, config, _) = fixture();
    for names in [
        vec!["joint.passive axle"],
        vec!["joint.passive axle", "joint.right axle", "joint.right axle"],
        vec!["joint.passive axle", "joint.right axle", "missing"],
    ] {
        let mut invalid = config.clone();
        invalid.independent_coordinates = Some(names.iter().map(|s| s.to_string()).collect());
        assert!(EmbeddedSession::new(scene.clone(), invalid, 0, CaptureMode::Latest).is_err());
    }
}

#[test]
fn detailed_motors_bind_by_dof_when_passive_coordinate_is_first() {
    let (scene, mut config, passive) = fixture();
    config.steps = 2;
    config.motors = Some(
        serde_json::from_value(json!({"residual_scales":[1.,1.,1.],
        "boundaries":[{"voltage_v":0.3,"winding_temperature_k":298.15},
                      {"voltage_v":-0.1,"winding_temperature_k":298.15}]}))
        .unwrap(),
    );
    let mut other_config = config.clone();
    other_config
        .independent_coordinates
        .as_mut()
        .unwrap()
        .reverse();
    let mut a = EmbeddedSession::new(scene.clone(), config, 1, CaptureMode::Latest).unwrap();
    let mut b = EmbeddedSession::new(scene, other_config, 1, CaptureMode::Latest).unwrap();
    a.advance(2).unwrap();
    b.advance(2).unwrap();
    let af = a.frame().unwrap();
    let bf = b.frame().unwrap();
    assert_eq!(af["motor_readings"].as_array().unwrap().len(), 2);
    assert!(af["joint_velocities"][passive].as_f64().unwrap() > 0.);
    for field in ["joint_positions", "joint_velocities"] {
        for (x, y) in af[field]
            .as_array()
            .unwrap()
            .iter()
            .zip(bf[field].as_array().unwrap())
        {
            assert!((x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-9);
        }
    }
    for (x, y) in af["motor_readings"]
        .as_array()
        .unwrap()
        .iter()
        .zip(bf["motor_readings"].as_array().unwrap())
    {
        for field in ["current_a", "shaft_torque_nm"] {
            assert!((x[field].as_f64().unwrap() - y[field].as_f64().unwrap()).abs() < 1e-9);
        }
    }
}

#[test]
fn transmission_can_make_an_actuated_coordinate_dependent() {
    let (mut scene, mut config, _) = fixture();
    scene
        .robot
        .transmissions
        .push(sim_domain_robot::model::Transmission {
            name: "coupled wheels".into(),
            driver_joint: "left axle".into(),
            driven_joint: "right axle".into(),
            ratio: 1.,
        });
    config.independent_coordinates =
        Some(vec!["joint.passive axle".into(), "joint.left axle".into()]);
    config.motors = Some(
        serde_json::from_value(json!({"residual_scales":[1.,1.,1.],
        "boundaries":[{"voltage_v":0.1,"winding_temperature_k":298.15},
                      {"voltage_v":0.2,"winding_temperature_k":298.15}]}))
        .unwrap(),
    );
    config.steps = 2;
    let mut runner = EmbeddedSession::new(scene, config, 1, CaptureMode::Latest).unwrap();
    runner.advance(2).unwrap();
    assert_eq!(
        runner.frame().unwrap()["motor_readings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn effective_servos_apply_and_report_torque_at_named_motor_coordinates() {
    let (scene, mut config, _) = fixture();
    let physical = Session::new(scene.clone(), 0).unwrap();
    let left = physical
        .robot
        .art
        .dofs()
        .position(|(_, d)| d.name == "joint.left axle")
        .unwrap();
    config.steps = 2;
    config.motors=Some(serde_json::from_value(json!({"residual_scales":[1.,1.,1.],
        "servos":[{"target_rad":0.01,"supply_voltage_v":6.,"winding_temperature_k":298.15},
                  {"target_rad":-0.02,"supply_voltage_v":6.,"winding_temperature_k":298.15}],
        "effective":{"version":1,"assumption_reference":"explicit synthetic servo response for binding test",
            "components":[
                {"dof":"joint.left axle","parameters":{"stiffness":0.1,"damping":0.001,"stall_torque":0.2,"no_load_speed":14.}},
                {"dof":"joint.right axle","parameters":{"stiffness":0.1,"damping":0.001,"stall_torque":0.2,"no_load_speed":14.}}
            ]}})).unwrap());
    let mut reordered = config.clone();
    reordered
        .independent_coordinates
        .as_mut()
        .unwrap()
        .reverse();
    let mut a = EmbeddedSession::new(scene.clone(), config, 0, CaptureMode::Latest).unwrap();
    let mut b = EmbeddedSession::new(scene, reordered, 0, CaptureMode::Latest).unwrap();
    a.advance(2).unwrap();
    b.advance(2).unwrap();
    let f = a.frame().unwrap();
    let other = b.frame().unwrap();
    let expected = 0.1 * (0.01 - f["joint_positions"][left].as_f64().unwrap())
        - 0.001 * f["joint_velocities"][left].as_f64().unwrap();
    assert!((f["motor_readings"][0]["shaft_torque_nm"].as_f64().unwrap() - expected).abs() < 1e-12);
    for key in ["joint_positions", "joint_velocities"] {
        for (x, y) in f[key]
            .as_array()
            .unwrap()
            .iter()
            .zip(other[key].as_array().unwrap())
        {
            assert!((x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-10);
        }
    }
}

#[test]
fn authored_imu_ticks_between_physics_steps_and_replays_with_passive_joints() {
    let (mut scene, mut config, _) = fixture();
    let original: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/baseline/robot.simrobot.json"
    ))
    .unwrap();
    scene.robot = serde_json::from_value(original).unwrap();
    config.step_s = 0.003;
    config.steps = 5;
    config.applied_generalized_loads.fill(0.);
    config.motors = Some(
        serde_json::from_value(json!({"residual_scales":[1.,1.,1.],"events":{},
        "boundaries":[{"voltage_v":0.,"winding_temperature_k":298.15},
                      {"voltage_v":0.,"winding_temperature_k":298.15}]}))
        .unwrap(),
    );
    let mut runner =
        EmbeddedSession::new(scene.clone(), config.clone(), 17, CaptureMode::Latest).unwrap();
    assert!(runner.frame().unwrap()["imu_samples"][0]["sample_time_s"].is_null());
    runner.advance(1).unwrap();
    assert!(runner.frame().unwrap()["imu_samples"][0]["sample_time_s"].is_null());
    runner.advance(1).unwrap();
    let sample = runner.frame().unwrap()["imu_samples"][0].clone();
    assert!((sample["sample_time_s"].as_f64().unwrap() - 0.005).abs() < 1e-12);
    runner.advance(1).unwrap();
    assert_eq!(runner.frame().unwrap()["imu_samples"][0], sample);
    runner.advance(2).unwrap();
    assert!(
        (runner.frame().unwrap()["imu_samples"][0]["sample_time_s"]
            .as_f64()
            .unwrap()
            - 0.015)
            .abs()
            < 1e-12
    );
    let (mut replay, steps) =
        EmbeddedSession::prepare_replay(runner.recording(), CaptureMode::Latest).unwrap();
    replay.advance(steps).unwrap();
    assert_eq!(runner.frame().unwrap(), replay.frame().unwrap());
    // Pure mechanical BE uses the same hybrid sensor deadlines and sampler.
    config.motors = None;
    config.mechanical_subdivision = Some(Default::default());
    let mut mechanical = EmbeddedSession::new(scene, config, 17, CaptureMode::Latest).unwrap();
    mechanical.advance(5).unwrap();
    assert!(
        (mechanical.frame().unwrap()["imu_samples"][0]["sample_time_s"]
            .as_f64()
            .unwrap()
            - 0.015)
            .abs()
            < 1e-12
    );
}

#[test]
fn stationary_imu_reports_specific_force_in_authored_sensor_axes() {
    let (mut scene, mut config, _) = fixture();
    let model: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/baseline/robot.simrobot.json"
    ))
    .unwrap();
    scene.robot = serde_json::from_value(model).unwrap();
    scene
        .robot
        .links
        .iter_mut()
        .find(|l| l.name == "chassis")
        .unwrap()
        .ground = true;
    let imu = scene
        .robot
        .sensors
        .iter_mut()
        .find(|s| s.kind == "imu")
        .unwrap();
    imu.noise.accel = 0.;
    imu.noise.gyro = 0.;
    imu.bias.accel = [0.; 3];
    imu.bias.gyro = [0.; 3];
    imu.bias_walk = 0.;
    imu.quantization.accel = 0.;
    imu.quantization.angle = 0.;
    imu.axes = [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]];
    config.applied_generalized_loads = vec![0.; 3];
    config.step_s = 0.003;
    config.steps = 5;
    config.mechanical_subdivision = Some(Default::default());
    let mut runner = EmbeddedSession::new(scene, config, 0, CaptureMode::Latest).unwrap();
    runner.advance(5).unwrap();
    let f = runner.frame().unwrap();
    let imu = &f["imu_samples"][0];
    for (actual, expected) in imu["specific_force_m_s2"]
        .as_array()
        .unwrap()
        .iter()
        .zip([9.81, 0., 0.])
    {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-9);
    }
    for actual in imu["angular_velocity_rad_s"].as_array().unwrap() {
        assert!(actual.as_f64().unwrap().abs() < 1e-10);
    }
}

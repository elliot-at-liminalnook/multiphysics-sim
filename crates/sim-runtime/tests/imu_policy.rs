use serde_json::{Value, json};
use sim_core::QuantityKind as Q;
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession},
    environment::{EmbeddedEnvironment, Task},
    imu_observation::{ImuChannel, ImuObserver},
    motion_data::MotionSnapshot,
    motion_forecast::*,
    session::Scene,
};

fn fixture() -> (Scene, Config, Task) {
    let robot: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/baseline/robot.simrobot.json"
    ))
    .unwrap();
    let scene=serde_json::from_value(json!({"version":1,"robot":robot,
        "options":{"contact":false,"flex":false},"period_s":0.003,"duration_s":0.03,
        "controller":{"sources":{"entry":"imu-controller.rhai","files":{"imu-controller.rhai":include_str!("../../../examples/wheeled-robot/imu-controller.rhai")}},
        "parameters":{},"inputs":[{"name":"command.left","kind":"Angle","lower":-0.1,"upper":0.1,"initial":0.},
        {"name":"command.right","kind":"Angle","lower":-0.1,"upper":0.1,"initial":0.}]}})).unwrap();
    let config = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.config.json"
    ))
    .unwrap();
    let task = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.task.json"
    ))
    .unwrap();
    (scene, config, task)
}

#[test]
fn scheduled_sensors_drive_rhai_and_replay_through_the_environment() {
    let (scene, config, task) = fixture();
    let original = json!(scene.robot);
    let mut env =
        EmbeddedEnvironment::new(scene.clone(), config.clone(), task.clone(), 17).unwrap();
    let initial = env.transition().clone();
    assert!(initial.imu_samples[0].sample_time_s.is_none());
    assert_eq!(&initial.observations[3..], &[0.01, -0.02]); // motor order, not chart order
    assert_eq!(
        env.metadata()["policy_contract"]["authored_imu_observations"]["component"],
        "robot.articulated"
    );
    let mut acted_on_sample = false;
    for i in 0..10 {
        let before = env.transition().clone();
        let action = [i as f64 * 0.0001, -i as f64 * 0.0001];
        let expected = ImuChannel::Gx
            .read(&before.imu_samples[0], before.time_s)
            .unwrap();
        let transition = env.step(&action).unwrap();
        let frame = env.frame().unwrap();
        let sensors = &frame["policy"]["observations"];
        assert_eq!(sensors["imu.body imu.gx"], expected);
        assert_eq!(
            sensors["imu.body imu.available"],
            if before.imu_samples[0].sample_time_s.is_some() {
                1.
            } else {
                0.
            }
        );
        assert_eq!(
            sensors["imu.body imu.age_s"],
            ImuChannel::Age
                .read(&before.imu_samples[0], before.time_s)
                .unwrap()
        );
        assert!(
            (frame["policy"]["targets"]["left axle.target"]
                .as_f64()
                .unwrap()
                - action[0]
                - 0.001 * expected)
                .abs()
                < 1e-14
        );
        acted_on_sample |= expected != 0.;
        assert_eq!(json!(transition.imu_samples), frame["imu_samples"]);
        assert_eq!(
            MotionSnapshot::from_frame(&frame).unwrap().imu_samples,
            transition.imu_samples
        );
    }
    assert!(acted_on_sample);
    assert!(env.transition().truncated && !env.transition().terminated);
    assert_eq!(json!(env.recording().scene.robot), original);
    let final_transition = env.transition().clone();
    let (mut replay, actions) = env.prepare_replay(env.episode_recording()).unwrap();
    for action in actions {
        replay.step(&action).unwrap();
    }
    assert_eq!(replay.transition(), &final_transition);
    assert_eq!(env.reset(17).unwrap(), initial);
    // Reordering the passive/actuated chart must not relabel state or references.
    let mut reordered = config;
    reordered
        .independent_coordinates
        .as_mut()
        .unwrap()
        .reverse();
    let mut other = EmbeddedEnvironment::new(scene, reordered, task, 17).unwrap();
    for _ in 0..10 {
        let a = env.step(&[0., 0.]).unwrap();
        let b = other.step(&[0., 0.]).unwrap();
        for (x, y) in a.observations.iter().zip(b.observations) {
            assert!((x - y).abs() < 1e-10);
        }
    }
}

#[test]
fn initial_motor_angles_follow_resolved_actuator_coordinates_including_transmissions() {
    let (mut scene, mut config, _) = fixture();
    config.initial_coordinates = Some(vec![0.03, -0.01, 0.02]);
    let runner =
        EmbeddedSession::new(scene.clone(), config.clone(), 0, CaptureMode::Latest).unwrap();
    let frame = runner.frame().unwrap();
    for m in frame["motor_readings"].as_array().unwrap() {
        assert!(
            m["shaft_torque_nm"].as_f64().unwrap().abs() < 1e-12,
            "initial motor preload: {m}"
        );
    }
    scene
        .robot
        .transmissions
        .push(sim_domain_robot::model::Transmission {
            name: "test wheel coupling".into(),
            driver_joint: "left axle".into(),
            driven_joint: "right axle".into(),
            ratio: 2.,
        });
    config.independent_coordinates =
        Some(vec!["joint.passive axle".into(), "joint.left axle".into()]);
    config.initial_coordinates = Some(vec![0.03, 0.02]);
    let mut runner = EmbeddedSession::new(scene, config, 0, CaptureMode::Latest).unwrap();
    let f = runner.frame().unwrap();
    for m in f["motor_readings"].as_array().unwrap() {
        assert!(m["shaft_torque_nm"].as_f64().unwrap().abs() < 1e-12);
    }
    let motor_q: Vec<_> = runner
        .motor_joint_indices()
        .iter()
        .map(|i| f["joint_positions"][*i].as_f64().unwrap())
        .collect();
    assert!((motor_q[0] - 2. * motor_q[1]).abs() < 1e-12);
    runner.advance(1).unwrap();
}

#[test]
fn passive_prismatic_observations_have_length_units_and_no_actuator_reference() {
    let (mut scene, mut config, mut task) = fixture();
    // Explicit synthetic topology variation, not a modification of the CAD artifact.
    let mut robot = json!(scene.robot);
    let joint = robot["joints"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|j| j["name"] == "passive axle")
        .unwrap();
    joint["type"] = json!("prismatic");
    scene.robot = serde_json::from_value(robot).unwrap();
    config.independent_coordinates.as_mut().unwrap()[0] = "slide.passive axle".into();
    task.observations[0].source = sim_runtime::environment::ObservationSource::CoordinatePosition {
        coordinate: "slide.passive axle".into(),
    };
    task.observations.push(serde_json::from_value(json!({"name":"passive.speed","source":{"kind":"coordinate_velocity","coordinate":"slide.passive axle"}})).unwrap());
    let mut env = EmbeddedEnvironment::new(scene.clone(), config.clone(), task.clone(), 0).unwrap();
    assert_eq!(env.contract()["observations"][0]["unit"], "m");
    assert_eq!(env.contract()["observations"][5]["unit"], "m/s");
    env.step(&[0., 0.]).unwrap();
    task.observations[0].source = sim_runtime::environment::ObservationSource::ReferencePosition {
        coordinate: "slide.passive axle".into(),
    };
    assert!(
        EmbeddedEnvironment::new(scene, config, task, 0)
            .err()
            .unwrap()
            .contains("no actuator reference")
    );
}

fn capture() -> (Value, ForecastRecipe) {
    let (scene, config, task) = fixture();
    let mut env = EmbeddedEnvironment::new(scene, config, task, 0).unwrap();
    let mut frames = vec![env.frame().unwrap()];
    for _ in 0..10 {
        env.step(&[0.001, -0.001]).unwrap();
        frames.push(env.frame().unwrap());
    }
    let recipe = ForecastRecipe {
        expected_cad_sha256: env.recording().scene.robot.source["cad_sha256"]
            .as_str()
            .unwrap()
            .into(),
        reference_link: "chassis".into(),
        axes: vec![MotionAxis::Link {
            name: "chassis.x".into(),
            link: "chassis".into(),
            axis: 0,
        }],
        physics_context:None, controller_context:None, controller_inputs:vec![], actuator_targets: vec!["left axle.target".into(), "right axle.target".into()],
        horizons_steps: vec![1, 2],
        period_s: 0.003,
        reference: KinematicReference::ConstantAcceleration,
        terrain_relative_links: vec![],
        imu_observations: vec!["body imu".into()],
    };
    (
        json!({"error":null,"frames":frames,"metadata":env.metadata(),"recording":env.recording()}),
        recipe,
    )
}

#[test]
fn forecasts_use_current_sensor_samples_and_causal_actions_with_typed_masks() {
    let (capture, recipe) = capture();
    let samples = samples_from_capture(&capture, &recipe, 0., 0.03).unwrap();
    let model = TrajectoryForecaster::initialize(recipe.clone(), &samples, 4, 7).unwrap();
    model.validate().unwrap();
    let offset = recipe.future_action_offset();
    assert_eq!(offset, 19);
    for (i, sample) in samples.iter().enumerate() {
        let current = MotionSnapshot::from_frame(&capture["frames"][i + 1]).unwrap();
        for (j, c) in ImuChannel::ALL.iter().enumerate() {
            assert_eq!(
                sample.inputs[9 + j],
                c.read(&current.imu_samples[0], current.time_s).unwrap()
            );
            assert_eq!(model.network.features[9 + j].kind, c.kind());
        }
        assert_eq!(
            sample.inputs[offset],
            capture["frames"][i + 2]["policy"]["targets"]["left axle.target"]
                .as_f64()
                .unwrap()
        );
    }
    assert_eq!(samples[0].inputs[15], 0.);
    assert_eq!(samples[1].inputs[15], 1.);
    // Changing only a future sensor value cannot enter an earlier input window.
    let mut changed = capture.clone();
    changed["frames"][3]["imu_samples"][0]["specific_force_m_s2"][0] = json!(1234.);
    let next = samples_from_capture(&changed, &recipe, 0., 0.03).unwrap();
    assert_eq!(samples[0].inputs, next[0].inputs);
    assert_eq!(samples[0].targets, next[0].targets);
    assert_ne!(samples[2].inputs, next[2].inputs);
    changed["frames"][3]["imu_samples"][0]["sample_time_s"] = json!(100.);
    assert!(
        samples_from_capture(&changed, &recipe, 0., 0.03)
            .unwrap_err()
            .contains("sample clock")
    );
    let mut missing = capture.clone();
    missing["recording"]["scene"]["robot"]["sensors"] = json!([]);
    assert!(
        samples_from_capture(&missing, &recipe, 0., 0.03)
            .unwrap_err()
            .contains("provenance")
    );
}

#[test]
fn sensor_bindings_reject_missing_ambiguous_and_non_imu_definitions() {
    let (scene, config, _) = fixture();
    for names in [
        vec!["missing".into()],
        vec!["left encoder".into()],
        vec!["body imu".into(), "body imu".into()],
    ] {
        let mut c = config.clone();
        c.policy.as_mut().unwrap().imu_observations = names;
        assert!(EmbeddedSession::new(scene.clone(), c, 0, CaptureMode::Latest).is_err());
    }
    let mut duplicate = scene.clone();
    duplicate
        .robot
        .sensors
        .push(duplicate.robot.sensors.last().unwrap().clone());
    assert!(EmbeddedSession::new(duplicate, config, 0, CaptureMode::Latest).is_err());
    let raw = sim_runtime::session::Session::new(scene, 0).unwrap();
    let observer = ImuObserver::new(&raw.robot.art, &["body imu".into()]).unwrap();
    let catalogue = sim_script::catalogue(&sim_runtime::registry());
    let registered = catalogue
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "robot.articulated")
        .unwrap();
    for c in &observer.channels()[..6] {
        let suffix = c.name.rsplit('.').next().unwrap();
        let port = registered["ports"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == format!("imu.*.{suffix}"))
            .unwrap();
        assert!(port.to_string().contains(c.kind.unit()), "{port}");
    }
    assert_eq!(observer.channels()[6].kind, Q::Dimensionless);
    assert_eq!(observer.channels()[7].kind, Q::Time);
}

#[test]
fn neural_and_online_prediction_consume_the_same_committed_sensor_contract() {
    let (training, mut recipe) = capture();
    recipe.horizons_steps = vec![1];
    let samples = samples_from_capture(&training, &recipe, 0., 0.03).unwrap();
    let head = TrajectoryForecaster::initialize(recipe, &samples, 4, 7).unwrap();
    let (scene, mut config, task) = fixture();
    let policy = config.policy.as_mut().unwrap();
    policy.trajectory_forecast = Some(sim_runtime::predictive_policy::ForecastBundle {
        version: 1,
        heads: vec![head],
    });
    policy.neural_residual=Some(serde_json::from_value(json!({"version":1,
        "features":[{"source":"imu.body imu.gx","kind":"AngularVelocity","center":0,"scale":1,"clip":10},
            {"source":"imu.body imu.available","kind":"Dimensionless","center":0,"scale":1,"clip":1},
            {"source":"forecast.valid","kind":"Dimensionless","center":0,"scale":1,"clip":1}],
        "outputs":[{"target":"left axle.target","kind":"Angle","scale":0.001},
            {"target":"right axle.target","kind":"Angle","scale":0.001}],
        "layers":[{"weights":[[0.5,0,0],[0,0.5,0]],"biases":[0,0]}]})).unwrap());
    let mut missing_mask = config.clone();
    let network = missing_mask
        .policy
        .as_mut()
        .unwrap()
        .neural_residual
        .as_mut()
        .unwrap();
    network.features.remove(1);
    for row in &mut network.layers[0].weights {
        row.remove(1);
    }
    assert!(
        EmbeddedEnvironment::new(scene.clone(), missing_mask, task.clone(), 0)
            .err()
            .unwrap()
            .contains("sensor actor must consume")
    );
    let mut env = EmbeddedEnvironment::new(scene, config, task, 0).unwrap();
    for i in 0..10 {
        let old = env.transition().clone();
        let gx = ImuChannel::Gx
            .read(&old.imu_samples[0], old.time_s)
            .unwrap();
        let available = ImuChannel::Available
            .read(&old.imu_samples[0], old.time_s)
            .unwrap();
        env.step(&[0., 0.]).unwrap();
        let frame = env.frame().unwrap();
        let policy = &frame["policy"];
        assert!(
            (policy["neural_residual"]["left axle.target"]
                .as_f64()
                .unwrap()
                - 0.001 * (0.5 * gx.clamp(-10., 10.)).tanh())
            .abs()
                < 1e-14
        );
        assert!(
            (policy["neural_residual"]["right axle.target"]
                .as_f64()
                .unwrap()
                - 0.001 * (0.5 * available).tanh())
            .abs()
                < 1e-14
        );
        if i > 0 {
            let forecast = &policy["trajectory_forecast"];
            assert_eq!(forecast["history_valid"], true);
            let inputs = forecast["inputs"].as_array().unwrap();
            for (j, c) in ImuChannel::ALL.iter().enumerate() {
                assert_eq!(
                    inputs[9 + j],
                    c.read(&old.imu_samples[0], old.time_s).unwrap()
                );
            }
        }
    }
}

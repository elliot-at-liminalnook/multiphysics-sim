use serde_json::{Value, json};
use sim_core::QuantityKind as Q;
use sim_runtime::{
    embedded::Config,
    environment::{EmbeddedEnvironment, Task},
    forecast_actions,
    motion_data::MotionSnapshot,
    motion_forecast::*,
    session::Scene,
};

fn fixture() -> (Scene, Config, Task) {
    let config: Config = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.config.json"
    ))
    .unwrap();
    let task = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.task.json"
    ))
    .unwrap();
    let robot: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/baseline/robot.simrobot.json"
    ))
    .unwrap();
    let inputs: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/velocity-controller.inputs.json"
    ))
    .unwrap();
    let scene=serde_json::from_value(json!({"version":1,"robot":robot,"options":{"contact":false,"flex":false},
        "period_s":0.003,"duration_s":0.03,"controller":{"inputs":inputs,
        "parameters":{"period_s":0.003,"initial_left":0.01,"initial_right":-0.02},
        "sources":{"entry":"velocity-controller.rhai","files":{"velocity-controller.rhai":include_str!("../../../examples/wheeled-robot/velocity-controller.rhai")}}}})).unwrap();
    (scene, config, task)
}
fn capture() -> (Value, ForecastRecipe, Vec<Vec<f64>>) {
    let (s, c, t) = fixture();
    let mut env = EmbeddedEnvironment::new(s, c, t, 3).unwrap();
    let actions = (0..10)
        .map(|i| vec![0.03 * i as f64, 0.1 - 0.02 * i as f64])
        .collect::<Vec<_>>();
    let mut frames = vec![env.frame().unwrap()];
    for a in &actions {
        env.step(a).unwrap();
        frames.push(env.frame().unwrap());
    }
    let mut inputs = env.inputs().to_vec();
    inputs.reverse();
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
        controller_context: Some(
            forecast_actions::ControllerContext::from_runtime(
                &env.recording().scene,
                &env.recording().config,
            )
            .unwrap(),
        ),
        controller_inputs: inputs,
        physics_context: Some(
            sim_runtime::physics_context::PhysicsContext::from_recording(&env.recording()).unwrap(),
        ),
        actuator_targets: vec![],
        horizons_steps: vec![1, 2],
        period_s: 0.003,
        reference: KinematicReference::ConstantAcceleration,
        terrain_relative_links: vec![],
        imu_observations: vec!["body imu".into()],
    };
    (
        json!({"error":null,"frames":frames,"recording":env.recording(),"metadata":env.metadata()}),
        recipe,
        actions,
    )
}

#[test]
fn typed_velocity_actions_match_recorded_schedule_and_read_only_online_prediction() {
    let (capture, recipe, actions) = capture();
    let samples = samples_from_capture(&capture, &recipe, 0., 0.03).unwrap();
    assert_eq!(samples.len(), 8);
    let model = TrajectoryForecaster::initialize(recipe.clone(), &samples, 4, 7).unwrap();
    assert_eq!(model.version, 2);
    let mut wrong_version = model.clone();
    wrong_version.version = 1;
    assert!(wrong_version.validate().is_err());
    let offset = recipe.future_action_offset();
    for (i, s) in samples.iter().enumerate() {
        assert_eq!(
            &s.inputs[offset - 2..offset],
            &[actions[i][1], actions[i][0]]
        );
        assert_eq!(
            &s.inputs[offset..offset + 2],
            &[actions[i + 1][1], actions[i + 1][0]]
        );
        assert_eq!(model.network.features[offset].kind, Q::AngularVelocity);
        assert_eq!(
            model.network.features[offset].source,
            "action.1.command.right_speed"
        );
    }
    let (scene, config, task) = fixture();
    let mut env = EmbeddedEnvironment::new(scene, config, task, 3).unwrap();
    let mut old = MotionSnapshot::from_frame(&env.frame().unwrap()).unwrap();
    for (i, s) in samples.iter().enumerate() {
        env.step(&actions[i]).unwrap();
        let frame = env.frame().unwrap();
        let recording = json!(env.episode_recording());
        let p = env
            .predict_controller_trajectory(&model, &old, &actions[i + 1..i + 3])
            .unwrap();
        assert_eq!(p.inputs, s.inputs);
        assert_eq!(
            p.physics_context,
            model.recipe.physics_context.clone().unwrap()
        );
        assert_eq!(p.prior, s.prior);
        assert_eq!(p.prediction, s.prior);
        assert_eq!(p.actions.channels[0].name, "command.right_speed");
        assert_eq!(
            p.actions.future[0],
            vec![actions[i + 1][1], actions[i + 1][0]]
        );
        let mut after = env.frame().unwrap();
        let mut before = frame.clone();
        for f in [&mut after, &mut before] {
            f.as_object_mut().unwrap().remove("stepping_wall_s");
        }
        assert_eq!(after, before);
        assert_eq!(json!(env.episode_recording()), recording);
        old = MotionSnapshot::from_frame(&frame).unwrap();
    }
    let (mut scene, config, task) = fixture();
    scene.controller.as_mut().unwrap().inputs.reverse();
    let mut reordered = EmbeddedEnvironment::new(scene, config, task, 3).unwrap();
    let mut old = MotionSnapshot::from_frame(&reordered.frame().unwrap()).unwrap();
    for (i, s) in samples.iter().enumerate() {
        reordered.step(&[actions[i][1], actions[i][0]]).unwrap();
        let future = actions[i + 1..i + 3]
            .iter()
            .map(|a| vec![a[1], a[0]])
            .collect::<Vec<_>>();
        let prediction = reordered
            .predict_controller_trajectory(&model, &old, &future)
            .unwrap();
        assert_eq!(prediction.inputs, s.inputs);
        assert_eq!(prediction.prior, s.prior);
        old = MotionSnapshot::from_frame(&reordered.frame().unwrap()).unwrap();
    }
}

#[test]
fn action_contracts_reject_mixed_layers_missing_channels_and_wrong_units() {
    let (capture, recipe, _) = capture();
    let mut wrong = recipe.clone();
    wrong.actuator_targets.push("left axle.target".into());
    assert!(wrong.validate().is_err());
    wrong = recipe.clone();
    wrong.controller_inputs[0].kind = Q::Angle;
    assert!(
        samples_from_capture(&capture, &wrong, 0., 0.03)
            .unwrap_err()
            .contains("units/bounds/initial")
    );
    wrong = recipe.clone();
    wrong.controller_inputs.pop();
    assert!(
        samples_from_capture(&capture, &wrong, 0., 0.03)
            .unwrap_err()
            .contains("complete controller input")
    );
    wrong = recipe.clone();
    wrong.controller_inputs[0].upper = 2.;
    assert!(
        samples_from_capture(&capture, &wrong, 0., 0.03)
            .unwrap_err()
            .contains("units/bounds/initial")
    );
    wrong = recipe.clone();
    wrong.controller_inputs[1].name = wrong.controller_inputs[0].name.clone();
    assert!(wrong.validate().is_err());
    let mut bad = capture.clone();
    bad["frames"][3]["policy_inputs"][0] = json!(0.9);
    assert!(
        samples_from_capture(&bad, &recipe, 0., 0.03)
            .unwrap_err()
            .contains("disagree")
    );
    let mut changed_program = capture.clone();
    changed_program["recording"]["scene"]["controller"]["parameters"]["period_s"] = json!(0.006);
    assert!(
        samples_from_capture(&changed_program, &recipe, 0., 0.03)
            .unwrap_err()
            .contains("context mismatch")
    );
    let mut changed_policy = capture.clone();
    changed_policy["recording"]["config"]["policy"]["target_bounds_rad"]["joint.left axle"] =
        json!([-0.5, 0.5]);
    assert!(
        samples_from_capture(&changed_policy, &recipe, 0., 0.03)
            .unwrap_err()
            .contains("context mismatch")
    );
}

#[test]
fn event_reconstruction_rejects_hidden_changes_and_preserves_causal_prefixes() {
    let (capture, recipe, _) = capture();
    let record: sim_runtime::embedded::EmbeddedRecording =
        serde_json::from_value(capture["recording"].clone()).unwrap();
    let frames = capture["frames"].as_array().unwrap();
    let coarse = frames.iter().step_by(2).cloned().collect::<Vec<_>>();
    assert!(
        forecast_actions::from_recording(&record, &coarse, &recipe.controller_inputs)
            .unwrap_err()
            .contains("inside a forecast interval")
    );
    let mut changed = record.clone();
    changed.input_events.last_mut().unwrap().values[0] = 0.8;
    let without_endpoint = frames
        .iter()
        .map(|f| json!({"time_s":f["time_s"]}))
        .collect::<Vec<_>>();
    let before =
        forecast_actions::from_recording(&record, &without_endpoint, &recipe.controller_inputs)
            .unwrap();
    let after =
        forecast_actions::from_recording(&changed, &without_endpoint, &recipe.controller_inputs)
            .unwrap();
    assert_eq!(&before[..9], &after[..9]);
    assert_ne!(before[9], after[9]);
    changed.input_events[1].at_step = changed.input_events[0].at_step;
    assert!(
        forecast_actions::from_recording(&changed, frames, &recipe.controller_inputs)
            .unwrap_err()
            .contains("schedule")
    );
    // A prefix at three 3-ms steps exercises floating-point clock conversion.
    let mut prefix = record;
    prefix.completed_steps = 3;
    prefix.input_events.retain(|e| e.at_step < 3);
    assert_eq!(
        forecast_actions::from_recording(&prefix, &frames[..4], &recipe.controller_inputs)
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn invalid_prediction_queries_preserve_the_environment() {
    let (capture, recipe, actions) = capture();
    let samples = samples_from_capture(&capture, &recipe, 0., 0.03).unwrap();
    let model = TrajectoryForecaster::initialize(recipe, &samples, 4, 1).unwrap();
    let (s, c, t) = fixture();
    let mut env = EmbeddedEnvironment::new(s, c, t, 0).unwrap();
    let old = MotionSnapshot::from_frame(&env.frame().unwrap()).unwrap();
    assert!(
        env.predict_controller_trajectory(&model, &old, &actions[1..3])
            .unwrap_err()
            .contains("history interval")
    );
    env.step(&actions[0]).unwrap();
    let recorded = json!(env.episode_recording());
    let mut future = actions[1..3].to_vec();
    future[0][0] = 2.;
    assert!(
        env.predict_controller_trajectory(&model, &old, &future)
            .unwrap_err()
            .contains("bounds")
    );
    let mut invalid = model.clone();
    invalid.recipe.expected_cad_sha256 = "different".into();
    assert!(
        env.predict_controller_trajectory(&invalid, &old, &actions[1..3])
            .unwrap_err()
            .contains("CAD")
    );
    let mut stale = old.clone();
    stale.time_s = -0.003;
    assert!(
        env.predict_controller_trajectory(&model, &stale, &actions[1..3])
            .is_err()
    );
    let mut other = old;
    other.joint_positions.push(0.);
    other.joint_velocities.push(0.);
    assert!(
        env.predict_controller_trajectory(&model, &other, &actions[1..3])
            .unwrap_err()
            .contains("topology")
    );
    assert_eq!(json!(env.episode_recording()), recorded);
}

#[test]
fn physics_profiles_cover_world_solver_actuators_and_source_but_allow_new_episode_state() {
    use sim_runtime::physics_context::{PhysicsContext, RuntimeIdentity};
    let (scene, config, _) = fixture();
    let expected = PhysicsContext::from_runtime(&scene, &config).unwrap();
    assert_eq!(expected.runtime, RuntimeIdentity::current());
    let mut changed = scene.clone();
    changed.robot.gravity[2] = -8.;
    assert!(
        expected
            .matches(&PhysicsContext::from_runtime(&changed, &config).unwrap())
            .unwrap_err()
            .contains("/scene/robot/gravity")
    );
    changed = scene.clone();
    changed.options.contact = true;
    assert!(
        expected
            .matches(&PhysicsContext::from_runtime(&changed, &config).unwrap())
            .unwrap_err()
            .contains("/scene/options")
    );
    let mut changed = config.clone();
    changed.step_s /= 2.;
    changed.steps *= 2;
    assert!(
        expected
            .matches(&PhysicsContext::from_runtime(&scene, &changed).unwrap())
            .unwrap_err()
            .contains("/config/step_s")
    );
    changed = config.clone();
    changed.motors.as_mut().unwrap().servos.as_mut().unwrap()[0].supply_voltage_v += 0.5;
    assert!(
        expected
            .matches(&PhysicsContext::from_runtime(&scene, &changed).unwrap())
            .unwrap_err()
            .contains("/config/motors")
    );
    changed = config.clone();
    changed.initial_base_translation_m = Some([0.2, 0.1, 0.]);
    changed.initial_coordinates = Some(vec![0.1, 0.2, 0.3]);
    changed.initial_base_rotation_vector_rad = Some([0., 0., 0.2]);
    changed.steps *= 3;
    changed.report_every *= 2;
    changed.profile_solver = true;
    changed.trace_trials = 3;
    changed.audit_contact_steps = true;
    let mut new_scene = scene.clone();
    new_scene.duration_s *= 3.;
    new_scene.controller.as_mut().unwrap().inputs.reverse();
    assert_eq!(
        expected,
        PhysicsContext::from_runtime(&new_scene, &changed).unwrap()
    );
    let mut old = expected.clone();
    old.runtime.library_source_blake3 = "0".repeat(64);
    assert!(
        old.matches(&expected)
            .unwrap_err()
            .contains("source/features")
    );
    let mut missing = expected.clone();
    missing.sections.remove("/config/step_s");
    assert!(missing.validate().is_err());
    let bytes = serde_json::to_vec(&expected).unwrap();
    assert!(bytes.len() < 10_000);
    assert_eq!(
        serde_json::from_slice::<PhysicsContext>(&bytes).unwrap(),
        expected
    );
}

#[test]
fn bound_capture_labels_reject_unknown_sources_changed_profiles_and_missing_bindings() {
    let (capture, recipe, _) = capture();
    let mut legacy = capture.clone();
    legacy["recording"]
        .as_object_mut()
        .unwrap()
        .remove("runtime_identity");
    assert!(
        samples_from_capture(&legacy, &recipe, 0., 0.03)
            .unwrap_err()
            .contains("recorded runtime source identity")
    );
    let mut wrong = recipe.clone();
    wrong.physics_context = None;
    assert!(
        wrong
            .validate()
            .unwrap_err()
            .contains("explicit physics context")
    );
    let mut changed = capture.clone();
    changed["recording"]["runtime_identity"]["library_source_blake3"] = json!("0".repeat(64));
    assert!(
        samples_from_capture(&changed, &recipe, 0., 0.03)
            .unwrap_err()
            .contains("source/features")
    );
    changed = capture.clone();
    changed["recording"]["config"]["motors"]["servos"][0]["supply_voltage_v"] = json!(5.);
    assert!(
        samples_from_capture(&changed, &recipe, 0., 0.03)
            .unwrap_err()
            .contains("/config/motors")
    );
    // Different random realizations are part of the learning distribution, not a
    // different physics profile. We do not invent a deterministic future seed.
    changed = capture.clone();
    changed["recording"]["seed"] = json!(41);
    assert!(samples_from_capture(&changed, &recipe, 0., 0.03).is_ok());
}

#[test]
fn online_queries_reject_changed_physics_without_mutating_or_advancing_the_environment() {
    let (capture, recipe, actions) = capture();
    let samples = samples_from_capture(&capture, &recipe, 0., 0.03).unwrap();
    let model = TrajectoryForecaster::initialize(recipe, &samples, 4, 7).unwrap();
    let (scene, mut config, task) = fixture();
    config.step_s /= 2.;
    config.steps *= 2;
    let mut env = EmbeddedEnvironment::new(scene, config, task, 3).unwrap();
    let previous = MotionSnapshot::from_frame(&env.frame().unwrap()).unwrap();
    env.step(&actions[0]).unwrap();
    let before = json!(env.episode_recording());
    assert!(
        env.predict_controller_trajectory(&model, &previous, &actions[1..3])
            .unwrap_err()
            .contains("/config/step_s")
    );
    assert_eq!(before, json!(env.episode_recording()));
    assert_eq!(env.transition().time_s, 0.003);
    assert!(env.metadata()["physics_context"]["sections"].is_object());
}

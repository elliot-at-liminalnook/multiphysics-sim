use serde_json::json;
use sim_core::QuantityKind as Q;
use sim_domain_control::{
    neural::*,
    ppo::{GaussianDecision, GaussianExploration},
};
use sim_runtime::{
    embedded::Config,
    environment::{EmbeddedEnvironment, Task},
    motion_data::MotionSnapshot,
    motion_forecast::*,
    predictive_policy::*,
    session::Scene,
};

fn fixture() -> (Scene, Config, Task, ForecastBundle, Network) {
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    scene.robot.source["cad_sha256"] = json!("analytic-pendulum");
    let mut config: Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.policy.json"
    ))
    .unwrap();
    config.steps = 320;
    let task: Task = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.environment.json"
    ))
    .unwrap();
    let mut env = EmbeddedEnvironment::new(scene.clone(), config.clone(), task.clone(), 7).unwrap();
    let previous = MotionSnapshot::from_frame(&env.frame().unwrap()).unwrap();
    env.step(&[0.2]).unwrap();
    let current = MotionSnapshot::from_frame(&env.frame().unwrap()).unwrap();
    let heads = [1, 3]
        .iter()
        .map(|&h| {
            let recipe = ForecastRecipe {
                expected_cad_sha256: "analytic-pendulum".into(),
                imu_observations:vec![], terrain_relative_links: vec![],
                reference_link: "ground".into(),
                axes: vec![
                    MotionAxis::Joint {
                        name: "joint.pivot".into(),
                        index: 0,
                        position_kind: Q::Angle,
                    },
                    MotionAxis::Link {
                        name: "pendulum.x".into(),
                        link: "pendulum".into(),
                        axis: 0,
                    },
                ],
                physics_context:None, controller_context:None, controller_inputs:vec![], actuator_targets: vec!["pivot.target".into()],
                horizons_steps: vec![h],
                period_s: scene.period_s,
                reference: KinematicReference::ConstantVelocity,
            };
            let (inputs, prior) =
                forecast_input(&recipe, &previous, &current, &[0.2], &vec![vec![0.3]; h]).unwrap();
            TrajectoryForecaster::initialize(
                recipe,
                &[ForecastSample {
                    time_s: current.time_s,
                    inputs,
                    targets: prior.clone(),
                    prior,
                }],
                4,
                7,
            )
            .unwrap()
        })
        .collect();
    let bundle = ForecastBundle { version: 1, heads };
    let actor = Network {
        version: 1,
        features: vec![Feature {
            source: "pivot.angle".into(),
            subtract: None,
            kind: Q::Angle,
            center: 0.,
            scale: 1.,
            clip: 5.,
        }],
        outputs: vec![Output {
            target: "pivot.target".into(),
            kind: Q::Angle,
            scale: 1.,
        }],
        layers: vec![Layer {
            weights: vec![vec![0.]],
            biases: vec![0.],
        }],
    };
    (scene, config, task, bundle, actor)
}

#[test]
fn actuator_forecasts_can_bind_physics_without_binding_the_policy_or_changing_motion() {
    let (scene,config,task,legacy,actor)=fixture();
    let context=sim_runtime::physics_context::PhysicsContext::from_runtime(&scene,&config).unwrap();
    let mut bound=legacy.clone();
    for head in &mut bound.heads {head.recipe.physics_context=Some(context.clone());head.version=2;}
    bound.validate().unwrap();
    let mut old_config=config.clone();old_config.policy.as_mut().unwrap().neural_residual=Some(legacy.augment_actor(&actor).unwrap());
    old_config.policy.as_mut().unwrap().trajectory_forecast=Some(legacy);
    let mut bound_config=config.clone();bound_config.policy.as_mut().unwrap().neural_residual=Some(bound.augment_actor(&actor).unwrap());
    bound_config.policy.as_mut().unwrap().trajectory_forecast=Some(bound);
    let mut old=EmbeddedEnvironment::new(scene.clone(),old_config,task.clone(),7).unwrap();
    let mut current=EmbeddedEnvironment::new(scene.clone(),bound_config.clone(),task.clone(),7).unwrap();
    for action in [0.2,0.3,-0.2] {
        old.step(&[action]).unwrap();current.step(&[action]).unwrap();
        let a=old.frame().unwrap();let b=current.frame().unwrap();
        for key in ["joint_positions","joint_velocities","poses","contacts","motor_states"] {assert_eq!(a[key],b[key],"{key}");}
    }
    bound_config.step_s/=2.;bound_config.steps*=2;
    match EmbeddedEnvironment::new(scene,bound_config,task,7) {
        Err(error)=>assert!(error.contains("/config/step_s")),Ok(_)=>panic!("bound actuator model accepted a changed physics clock"),
    }
}

#[test]
fn terrain_inputs_match_recorded_world_and_online_runtime_without_changing_motion() {
    for grid in [false, true] {
        let (mut scene, mut config, task, old_bundle, actor) = fixture();
        scene.robot.world.floor_z = -10.;
        scene.robot.world.terrain = grid.then(|| sim_domain_robot::model::Terrain {
            origin: [-2., -2.],
            cell: 4.,
            dims: [2, 2],
            heights: vec![-10., -9.5, -9., -8.5],
        });
        config.steps = (task.period_s * 8. / config.step_s).round() as usize;
        let mut probe =
            EmbeddedEnvironment::new(scene.clone(), config.clone(), task.clone(), 7).unwrap();
        let previous = MotionSnapshot::from_frame(&probe.frame().unwrap()).unwrap();
        probe.step(&[0.2]).unwrap();
        let mut current = MotionSnapshot::from_frame(&probe.frame().unwrap()).unwrap();
        let links = vec!["ground".into(), "pendulum".into()];
        current
            .observe_ground(&links, |x, y| scene.robot.world.floor_height(x, y))
            .unwrap();
        let bundle = ForecastBundle {
            version: 1,
            heads: old_bundle
                .heads
                .iter()
                .map(|head| {
                    let mut recipe = head.recipe.clone();
                    recipe.terrain_relative_links = links.clone();
                    let h = recipe.horizons_steps[0];
                    let (inputs, prior) =
                        forecast_input(&recipe, &previous, &current, &[0.2], &vec![vec![0.2]; h])
                            .unwrap();
                    TrajectoryForecaster::initialize(
                        recipe,
                        &[ForecastSample {
                            time_s: current.time_s,
                            inputs,
                            targets: prior.clone(),
                            prior,
                        }],
                        4,
                        7,
                    )
                    .unwrap()
                })
                .collect(),
        };
        assert!(
            bundle
                .channels()
                .unwrap()
                .iter()
                .any(|c| c.name == "forecast.input.terrain.pendulum.height" && c.kind == Q::Length)
        );
        let mut plain =
            EmbeddedEnvironment::new(scene.clone(), config.clone(), task.clone(), 7).unwrap();
        let policy = config.policy.as_mut().unwrap();
        policy.neural_residual = Some(bundle.augment_actor(&actor).unwrap());
        policy.neural_command_saturation = true;
        policy.trajectory_forecast = Some(bundle.clone());
        let mut online = EmbeddedEnvironment::new(scene.clone(), config.clone(), task, 7).unwrap();
        let mut frames = vec![online.frame().unwrap()];
        for _ in 0..8 {
            plain.step(&[0.2]).unwrap();
            online.step(&[0.2]).unwrap();
            let frame = online.frame().unwrap();
            let reference = plain.frame().unwrap();
            for key in [
                "poses",
                "joint_positions",
                "joint_velocities",
                "servo_targets_rad",
                "contacts",
            ] {
                assert_eq!(frame[key], reference[key], "{key}");
            }
            frames.push(frame);
        }
        let capture = json!({"error":null, "frames":frames, "recording":online.recording(), "metadata":online.metadata()});
        for head in &bundle.heads {
            let samples =
                samples_from_capture(&capture, &head.recipe, 0., 8. * scene.period_s).unwrap();
            for sample in samples {
                let index = (sample.time_s / scene.period_s).round() as usize;
                let live = capture["frames"][index + 1]["policy"]["trajectory_forecast"]["inputs"]
                    .as_array()
                    .unwrap();
                let inputs: Vec<f64> = live[..sample.inputs.len()]
                    .iter()
                    .map(|x| x.as_f64().unwrap())
                    .collect();
                assert_eq!(inputs, sample.inputs);
                assert_eq!(sample.inputs[head.recipe.future_action_offset()], 0.2);
            }
        }
        let head = &bundle.heads[0];
        let mut missing = capture.clone();
        missing["recording"]["scene"]["robot"]["world"]
            .as_object_mut()
            .unwrap()
            .remove("floor_z");
        assert!(
            samples_from_capture(&missing, &head.recipe, 0., 8. * scene.period_s)
                .unwrap_err()
                .contains("explicit recorded")
        );
        let mut bad = capture.clone();
        bad["recording"]["scene"]["robot"]["world"]["terrain"] =
            json!({"origin":[0.,0.],"cell":0.,"dims":[2,2],"heights":[0.,0.,0.,0.]});
        assert!(samples_from_capture(&bad, &head.recipe, 0., 8. * scene.period_s).is_err());
        bad = capture.clone();
        bad["frames"][1]["floor_heights_m"] = json!({"pendulum":100.});
        assert!(
            samples_from_capture(&bad, &head.recipe, 0., 8. * scene.period_s)
                .unwrap_err()
                .contains("disagrees")
        );
        let (mut replay, actions) = online.prepare_replay(online.episode_recording()).unwrap();
        for a in actions {
            replay.step(&a).unwrap();
        }
        assert_eq!(
            online.frame().unwrap()["policy"],
            replay.frame().unwrap()["policy"]
        );
    }
}

#[test]
fn predictive_observations_preserve_zero_policy_physics_and_use_only_past_acceleration() {
    let (s, mut c, t, bundle, actor) = fixture();
    let mut baseline = EmbeddedEnvironment::new(s.clone(), c.clone(), t.clone(), 7).unwrap();
    c.policy.as_mut().unwrap().neural_residual = Some(bundle.augment_actor(&actor).unwrap());
    c.policy.as_mut().unwrap().trajectory_forecast = Some(bundle.clone());
    let mut predictive = EmbeddedEnvironment::new(s, c, t, 7).unwrap();
    let mut previous = MotionSnapshot::from_frame(&baseline.frame().unwrap()).unwrap();
    let mut previous_target = 0.2;
    for (i, action) in [0.3, -0.2, 0.4, 0.1].iter().enumerate() {
        let before = MotionSnapshot::from_frame(&baseline.frame().unwrap()).unwrap();
        assert_eq!(
            baseline.step(&[*action]).unwrap(),
            predictive.step(&[*action]).unwrap()
        );
        let a = baseline.frame().unwrap();
        let b = predictive.frame().unwrap();
        for key in [
            "poses",
            "joint_positions",
            "joint_velocities",
            "servo_targets_rad",
            "contacts",
        ] {
            assert_eq!(a[key], b[key], "{key}");
        }
        let f: ForecastObservation =
            serde_json::from_value(b["policy"]["trajectory_forecast"].clone()).unwrap();
        assert_eq!(f.history_valid, i > 0);
        assert_eq!(f.proposed_targets_rad, vec![vec![*action]; 3]);
        if i > 0 {
            let expected = bundle
                .predict_sequence(
                    &previous,
                    &before,
                    &[previous_target],
                    &vec![vec![*action]; 3],
                )
                .unwrap();
            for (x, y) in f.values().iter().zip(expected.values()) {
                assert!(
                    (x - y).abs() < 1e-10,
                    "online/offline motion mismatch: {x} {y}"
                );
            }
        }
        previous = before;
        previous_target = *action;
    }
    let record = predictive.episode_recording();
    let (mut replay, actions) = predictive.prepare_replay(record).unwrap();
    for a in actions {
        replay.step(&a).unwrap();
    }
    assert_eq!(
        predictive.frame().unwrap()["policy"],
        replay.frame().unwrap()["policy"]
    );
    predictive.reset(7).unwrap();
    predictive.step(&[0.3]).unwrap();
    assert_eq!(
        predictive.frame().unwrap()["policy"]["trajectory_forecast"]["history_valid"],
        false
    );
}

#[test]
fn future_commands_cannot_affect_earlier_head_and_ambiguous_models_are_rejected() {
    let (s, c, t, mut bundle, _) = fixture();
    let mut env = EmbeddedEnvironment::new(s, c, t, 7).unwrap();
    let previous = MotionSnapshot::from_frame(&env.frame().unwrap()).unwrap();
    env.step(&[0.2]).unwrap();
    let current = MotionSnapshot::from_frame(&env.frame().unwrap()).unwrap();
    for head in &mut bundle.heads {
        for f in &mut head.network.features {
            f.center = 0.;
            f.scale = 1.;
        }
        // A test oracle reads the last permitted action, so the long head must
        // respond while the short head remains causally isolated.
        let mut row = vec![0.; head.network.features.len()];
        *row.last_mut().unwrap() = 1.;
        head.network.layers = vec![Layer {
            weights: vec![row; head.network.outputs.len()],
            biases: vec![0.; head.network.outputs.len()],
        }];
    }
    let a = bundle
        .predict_sequence(&previous, &current, &[0.2], &vec![vec![0.1]; 3])
        .unwrap();
    let b = bundle
        .predict_sequence(
            &previous,
            &current,
            &[0.2],
            &[vec![0.1], vec![0.7], vec![0.8]],
        )
        .unwrap();
    assert_eq!(a.predictions[0], b.predictions[0]);
    assert_ne!(a.predictions[1], b.predictions[1]);
    bundle.heads[0].recipe.horizons_steps = vec![1, 3];
    assert!(bundle.validate().is_err());
}

#[test]
fn forecast_binding_checks_physical_contract_and_gaussian_likelihood_uses_augmented_inputs() {
    let (s, mut c, t, bundle, actor) = fixture();
    let augmented = bundle.augment_actor(&actor).unwrap();
    c.policy.as_mut().unwrap().trajectory_forecast = Some(bundle.clone());
    assert!(
        EmbeddedEnvironment::new(s.clone(), c.clone(), t.clone(), 7)
            .err()
            .unwrap()
            .contains("explicit neural")
    );
    c.policy.as_mut().unwrap().neural_residual = Some(actor);
    assert!(
        EmbeddedEnvironment::new(s.clone(), c.clone(), t.clone(), 7)
            .err()
            .unwrap()
            .contains("forecast.valid")
    );
    c.policy.as_mut().unwrap().neural_residual = Some(augmented.clone());
    for mutate in 0..3 {
        let mut bad = c.clone();
        let r = &mut bad
            .policy
            .as_mut()
            .unwrap()
            .trajectory_forecast
            .as_mut()
            .unwrap()
            .heads;
        for h in r {
            match mutate {
                0 => h.recipe.expected_cad_sha256 = "wrong".into(),
                1 => h.recipe.period_s *= 2.,
                _ => {
                    h.recipe.axes[0] = MotionAxis::Joint {
                        name: "joint.pivot".into(),
                        index: 1,
                        position_kind: Q::Angle,
                    }
                }
            }
        }
        assert!(EmbeddedEnvironment::new(s.clone(), bad, t.clone(), 7).is_err());
    }
    let exploration = GaussianExploration {
        standard_deviation: vec![0.1],
    };
    c.policy.as_mut().unwrap().neural_command_saturation = true;
    c.policy.as_mut().unwrap().neural_exploration = Some(exploration.clone());
    let mut env = EmbeddedEnvironment::new(s, c, t, 7).unwrap();
    for action in [0.2, 0.3] {
        env.step(&[action]).unwrap();
        let frame = env.frame().unwrap();
        let decision: GaussianDecision =
            serde_json::from_value(frame["policy"]["neural_decision"].clone()).unwrap();
        let values = &frame["policy"]["neural_observations"];
        let expected = augmented
            .features
            .iter()
            .map(|f| {
                let raw = values[&f.source].as_f64().unwrap()
                    - f.subtract
                        .as_ref()
                        .map_or(0., |s| values[s].as_f64().unwrap());
                ((raw - f.center) / f.scale).clamp(-f.clip, f.clip)
            })
            .collect::<Vec<_>>();
        assert_eq!(decision.inputs, expected);
        assert_eq!(
            decision.means,
            augmented.normalized_output(&expected, false).unwrap()
        );
        assert_eq!(
            decision.log_probability,
            exploration
                .log_probability(&decision.means, &decision.raw_actions)
                .unwrap()
        );
    }
}

#[test]
fn receding_forecast_search_applies_first_target_and_replays_command_expiry() {
    use sim_domain_control::{
        command_lease::CommandLeaseConfig, optimization::ProjectedAscentSettings,
    };
    use sim_runtime::{predictive_control::ForecastActionConfig, session::InputChannel};
    let (mut scene, mut config, task, _, _) = fixture();
    config.steps = 1280;
    for (name, kind, initial, lower, upper) in [
        ("drive.forward", Q::LinearVelocity, 1., -1., 1.),
        ("drive.lateral", Q::LinearVelocity, 0., -1., 1.),
        ("drive.sequence", Q::Dimensionless, 0., 0., 100.),
    ] {
        scene
            .controller
            .as_mut()
            .unwrap()
            .inputs
            .push(InputChannel {
                name: name.into(),
                kind,
                initial,
                lower,
                upper,
            });
    }
    let mut plain =
        EmbeddedEnvironment::new(scene.clone(), config.clone(), task.clone(), 7).unwrap();
    let previous = MotionSnapshot::from_frame(&plain.frame().unwrap()).unwrap();
    plain.step(&[0.2, 1., 0., 1.]).unwrap();
    let current = MotionSnapshot::from_frame(&plain.frame().unwrap()).unwrap();
    let recipe = ForecastRecipe {
        expected_cad_sha256: "analytic-pendulum".into(),
        imu_observations:vec![], terrain_relative_links: vec![],
        reference_link: "pendulum".into(),
        axes: (0..3)
            .map(|axis| MotionAxis::Link {
                name: format!("body.{}", ["x", "y", "z"][axis]),
                link: "pendulum".into(),
                axis,
            })
            .collect(),
        physics_context:None, controller_context:None, controller_inputs:vec![], actuator_targets: vec!["pivot.target".into()],
        horizons_steps: vec![3],
        period_s: 0.02,
        reference: KinematicReference::ConstantVelocity,
    };
    let (inputs, prior) =
        forecast_input(&recipe, &previous, &current, &[0.2], &vec![vec![0.2]; 3]).unwrap();
    let mut head = TrajectoryForecaster::initialize(
        recipe,
        &[ForecastSample {
            time_s: 0.02,
            inputs,
            targets: prior.clone(),
            prior,
        }],
        4,
        7,
    )
    .unwrap();
    for f in &mut head.network.features {
        f.center = 0.;
        f.scale = 1.;
    }
    for o in &mut head.network.outputs {
        o.scale = 1.;
    }
    let mut row = vec![0.; head.network.features.len()];
    let first = head
        .network
        .features
        .iter()
        .position(|f| f.source == "action.1.pivot.target")
        .unwrap();
    row[first] = 0.1;
    let mut weights = vec![vec![0.; row.len()]; head.network.outputs.len()];
    weights[0] = row;
    head.network.layers = vec![Layer {
        weights,
        biases: vec![0.; head.network.outputs.len()],
    }];
    config.policy.as_mut().unwrap().trajectory_forecast = Some(ForecastBundle {
        version: 1,
        heads: vec![head],
    });
    config.policy.as_mut().unwrap().forecast_action_search = Some(ForecastActionConfig {
        objective: Default::default(),
        optimizer: ProjectedAscentSettings {
            iterations: 3,
            step_fraction: 0.1,
            backtracks: 10,
        },
        forward_speed_input: "drive.forward".into(),
        lateral_speed_input: "drive.lateral".into(),
        packet_sequence_input: "drive.sequence".into(),
        command_lease: CommandLeaseConfig {
            period_s: 0.02,
            timeout_s: 0.04,
        },
        heading_offset_rad: 0.,
        warm_start: true,
        reference_proposal: None,
    });
    let mut env = EmbeddedEnvironment::new(scene.clone(), config.clone(), task.clone(), 7).unwrap();
    env.step(&[0.2, 1., 0., 1.]).unwrap();
    assert_eq!(
        env.frame().unwrap()["policy"]["forecast_action_search"]["reason"],
        "startup_history"
    );
    env.step(&[0.2, 1., 0., 2.]).unwrap();
    let frame = env.frame().unwrap();
    assert_eq!(frame["policy"]["forecast_action_search"]["active"], true);
    let f = &frame["policy"]["trajectory_forecast"];
    assert_eq!(
        frame["policy"]["targets"]["pivot.target"],
        f["proposed_targets_rad"][0][0]
    );
    assert!(f["proposed_targets_rad"][0][0].as_f64().unwrap() > 0.2);
    env.step(&[0.2, 1., 0., 2.]).unwrap();
    env.step(&[0.2, 1., 0., 2.]).unwrap();
    assert_eq!(
        env.frame().unwrap()["policy"]["forecast_action_search"]["reason"],
        "expired_command"
    );
    assert_eq!(
        env.frame().unwrap()["policy"]["targets"]["pivot.target"],
        0.2
    );
    env.step(&[0.2, 0., 0., 3.]).unwrap();
    assert_eq!(
        env.frame().unwrap()["policy"]["forecast_action_search"]["reason"],
        "zero_motion_request"
    );
    env.step(&[0.2, -1., 0., 4.]).unwrap();
    assert!(
        env.frame().unwrap()["policy"]["trajectory_forecast"]["proposed_targets_rad"][0][0]
            .as_f64()
            .unwrap()
            < 0.2
    );
    let record = env.episode_recording();
    let (mut replay, actions) = env.prepare_replay(record).unwrap();
    for a in actions {
        replay.step(&a).unwrap();
    }
    assert_eq!(
        env.frame().unwrap()["policy"],
        replay.frame().unwrap()["policy"]
    );
    env.reset(7).unwrap();
    env.step(&[0.2, 1., 0., 1.]).unwrap();
    assert_eq!(
        env.frame().unwrap()["policy"]["forecast_action_search"]["reason"],
        "startup_history"
    );
    let mut net_config = config.clone();
    net_config
        .policy
        .as_mut()
        .unwrap()
        .forecast_action_search
        .as_mut()
        .unwrap()
        .objective =
        sim_runtime::predictive_control::ForecastActionObjective::EpisodeNetDisplacement;
    let mut net_env = EmbeddedEnvironment::new(scene.clone(), net_config, task.clone(), 7).unwrap();
    let initial = MotionSnapshot::from_frame(&net_env.frame().unwrap()).unwrap();
    let body = initial.poses.iter().find(|p| p.name == "pendulum").unwrap();
    let origin = [body.position_m[0], body.position_m[1]];
    for i in 0..4 {
        let before = MotionSnapshot::from_frame(&net_env.frame().unwrap()).unwrap();
        net_env.step(&[0.2, 1., 0., i as f64 + 1.]).unwrap();
        let frame = net_env.frame().unwrap();
        let report = &frame["policy"]["forecast_action_search"];
        assert_eq!(report["episode_origin_xy_m"], json!(origin));
        if i > 0 {
            let prediction = &frame["policy"]["trajectory_forecast"]["predictions"][0];
            let local = [
                prediction[0].as_f64().unwrap(),
                prediction[3].as_f64().unwrap(),
                prediction[6].as_f64().unwrap(),
            ];
            let anchor = before.poses.iter().find(|p| p.name == "pendulum").unwrap();
            let expected =
                sim_runtime::predictive_control::forecast_net_progress(origin, anchor, local)
                    .unwrap()
                    .0;
            let observed = report["optimization"]["objectives"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .as_f64()
                .unwrap();
            assert!((expected - observed).abs() < 1e-12);
        }
    }
    let record = net_env.episode_recording();
    let (mut replay, actions) = net_env.prepare_replay(record).unwrap();
    for a in actions {
        replay.step(&a).unwrap();
    }
    assert_eq!(
        net_env.frame().unwrap()["policy"],
        replay.frame().unwrap()["policy"]
    );
    net_env.reset(7).unwrap();
    net_env.step(&[0.2, 1., 0., 1.]).unwrap();
    assert_eq!(
        net_env.frame().unwrap()["policy"]["forecast_action_search"]["episode_origin_xy_m"],
        json!(origin)
    );
    let mut seeded_config = config.clone();
    seeded_config
        .policy
        .as_mut()
        .unwrap()
        .forecast_action_search
        .as_mut()
        .unwrap()
        .reference_proposal = Some(sim_runtime::predictive_control::ForecastActionReference {
        expected_cad_sha256: "analytic-pendulum".into(),
        actuators: vec![sim_core::Channel {
            name: "pivot.target".into(),
            kind: Q::Angle,
        }],
        trajectory: sim_domain_control::trajectory::TrajectoryConfig {
            interpolation: Default::default(),
            keyframes: vec![
                sim_domain_control::trajectory::Keyframe {
                    time_s: 0.,
                    values: vec![0.6],
                },
                sim_domain_control::trajectory::Keyframe {
                    time_s: 0.16,
                    values: vec![0.92],
                },
            ],
        },
    });
    let mut seeded =
        EmbeddedEnvironment::new(scene.clone(), seeded_config.clone(), task.clone(), 7).unwrap();
    seeded.step(&[0.2, 1., 0., 1.]).unwrap();
    seeded.step(&[0.2, 1., 0., 2.]).unwrap();
    let frame = seeded.frame().unwrap();
    let report = &frame["policy"]["forecast_action_search"];
    assert_eq!(report["reference_proposal_selected"], true);
    assert_eq!(report["warm_start_selected"], false);
    let score = report["reference_proposal_displacement_m"]
        .as_f64()
        .unwrap();
    let held = report["held_proposal_displacement_m"].as_f64().unwrap();
    let direction_x = report["direction_reference"][0].as_f64().unwrap();
    assert!((score - held - direction_x * 0.1 * (0.64 - 0.2)).abs() < 1e-12);
    assert_eq!(
        report["optimization"]["objectives"][0].as_f64().unwrap(),
        score
    );
    let proposal = &frame["policy"]["trajectory_forecast"]["proposed_targets_rad"];
    assert!(proposal[0][0].as_f64().unwrap() > 0.64); // reference is an initializer, not a constraint
    assert!((proposal[1][0].as_f64().unwrap() - 0.68).abs() < 1e-14);
    assert!((proposal[2][0].as_f64().unwrap() - 0.72).abs() < 1e-14);
    seeded.step(&[0.2, -1., 0., 3.]).unwrap();
    let frame = seeded.frame().unwrap();
    let report = &frame["policy"]["forecast_action_search"];
    assert!(
        !report["reference_proposal_selected"]
            .as_bool()
            .unwrap_or(false)
    );
    assert!(
        report["reference_proposal_displacement_m"]
            .as_f64()
            .unwrap()
            < report["held_proposal_displacement_m"].as_f64().unwrap()
    );
    let (mut replay, actions) = seeded.prepare_replay(seeded.episode_recording()).unwrap();
    for a in actions {
        replay.step(&a).unwrap();
    }
    assert_eq!(
        seeded.frame().unwrap()["policy"],
        replay.frame().unwrap()["policy"]
    );
    seeded.reset(7).unwrap();
    seeded.step(&[0.2, 1., 0., 1.]).unwrap();
    seeded.step(&[0.2, 1., 0., 2.]).unwrap();
    assert_eq!(
        seeded.frame().unwrap()["policy"]["forecast_action_search"]["reference_proposal_selected"],
        true
    );
    for mutation in 0..5 {
        let mut bad = seeded_config.clone();
        let r = bad
            .policy
            .as_mut()
            .unwrap()
            .forecast_action_search
            .as_mut()
            .unwrap()
            .reference_proposal
            .as_mut()
            .unwrap();
        match mutation {
            0 => r.expected_cad_sha256 = "different".into(),
            1 => r.actuators[0].kind = Q::Length,
            2 => r.actuators[0].name = "missing".into(),
            3 => r.trajectory.keyframes[0].values[0] = 1.1,
            _ => {
                for k in &mut r.trajectory.keyframes {
                    k.values.push(0.);
                }
            }
        }
        assert!(EmbeddedEnvironment::new(scene.clone(), bad, task.clone(), 7).is_err());
    }
    let mut bad = config;
    bad.policy
        .as_mut()
        .unwrap()
        .forecast_action_search
        .as_mut()
        .unwrap()
        .forward_speed_input = "drive.sequence".into();
    assert!(
        EmbeddedEnvironment::new(scene, bad, task, 7)
            .err()
            .unwrap()
            .contains("typed")
    );
}

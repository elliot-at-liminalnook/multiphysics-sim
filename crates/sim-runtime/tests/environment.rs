use serde_json::json;
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession},
    environment::*,
    session::Scene,
};

fn fixture() -> (Scene, Config, Task) {
    let scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    let mut config: Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.policy.json"
    ))
    .unwrap();
    config.steps = 240;
    let task=serde_json::from_value(json!({
        "version":1,"observation_source":"ideal_runtime_teacher_only","period_s":0.02,
        "observations":[
            {"name":"angle","source":{"kind":"coordinate_position","coordinate":"joint.pivot"}},
            {"name":"speed","source":{"kind":"coordinate_velocity","coordinate":"joint.pivot"}},
            {"name":"height","source":{"kind":"body_position","link":"pendulum","axis":"z"}},
            {"name":"current","source":{"kind":"motor_current","motor":"servo"}}
        ],
        "rewards":[{"name":"tracking","observation":"angle","target":{"kind":"constant","value":0.3},"scale":0.2,"weight_per_s":2.0}],
        "termination_bounds":[]
    })).unwrap();
    (scene, config, task)
}

#[test]
fn teacher_motion_and_action_observations_use_current_frames_and_declared_units() {
    let (s, c, mut task) = fixture();
    for (name, source) in [
        ("local.vx", ObservationSource::BodyLocalVelocity { link: "pendulum".into(), axis: Axis::X }),
        ("world.wy", ObservationSource::BodyAngularVelocity { link: "pendulum".into(), axis: Axis::Y }),
        ("up.x", ObservationSource::BodyAxis { link: "pendulum".into(), body_axis: Axis::Z, world_axis: Axis::X }),
        ("command", ObservationSource::ControllerInput { name: "command.position".into() }),
        ("torque", ObservationSource::MotorTorque { motor: "servo".into() }),
    ] { task.observations.push(Observation { name: name.into(), source }); }
    let mut env = EmbeddedEnvironment::new(s.clone(), c.clone(), task.clone(), 0).unwrap();
    assert_eq!(env.transition().observations[7], 0.2);
    for (i, unit) in [(4,"m/s"),(5,"rad/s"),(6,"1"),(7,"rad"),(8,"N·m")] {
        assert_eq!(env.contract()["observations"][i]["unit"], unit);
    }
    let observed = env.step(&[0.6]).unwrap().observations;
    // This pendulum's COM is 60 mm below its Y-axis pivot. Its local X
    // velocity is -0.06 * angular velocity, even when its world axes rotate.
    assert!((observed[4] + 0.06 * observed[5]).abs() < 1e-10);
    assert!((observed[6] - observed[0].sin()).abs() < 1e-10);
    assert_eq!(observed[7], 0.6);
    assert_eq!(observed[8], env.frame().unwrap()["motor_readings"][0]["shaft_torque_nm"].as_f64().unwrap());
    assert!(observed[5].abs() > 1e-6);
    assert_eq!(env.reset(0).unwrap().observations[7], 0.2);

    let mut wrong_units = task.clone();
    wrong_units.rewards[0].target = Target::Observation { name: "torque".into() };
    assert!(EmbeddedEnvironment::new(s.clone(), c.clone(), wrong_units, 0).err().unwrap().contains("units"));
    task.observations[7].source = ObservationSource::ControllerInput { name: "missing".into() };
    assert!(EmbeddedEnvironment::new(s, c, task, 0).err().unwrap().contains("unknown controller input"));
}

#[test]
fn survival_and_failure_rewards_distinguish_task_failure_from_timeout() {
    let (s, c, mut task) = fixture();
    assert!(serde_json::to_value(&task).unwrap().get("termination_penalty").is_none());
    task.rewards.clear();
    task.survival_reward_per_s = 2.0;
    task.termination_penalty = 5.0;
    let mut env = EmbeddedEnvironment::new(s.clone(), c.clone(), task.clone(), 0).unwrap();
    assert_eq!(env.transition().reward, 0.0);
    for _ in 0..3 { assert_eq!(env.step(&[0.2]).unwrap().reward, 0.04); }
    assert!(env.transition().truncated && !env.transition().terminated);
    task.termination_bounds.push(TerminationBound { observation: "angle".into(), lower: 0.0, upper: 0.0 });
    let mut failed = EmbeddedEnvironment::new(s.clone(), c.clone(), task.clone(), 0).unwrap();
    let t = failed.step(&[0.8]).unwrap();
    assert!(t.terminated);
    assert_eq!(t.reward, 0.04 - 5.0);
    assert!(failed.step(&[0.8]).is_err());
    assert_eq!(failed.reset(0).unwrap().reward, 0.0);
    task.termination_penalty = -1.0;
    assert!(EmbeddedEnvironment::new(s, c, task, 0).is_err());
}

#[test]
fn held_actions_match_the_production_runtime_and_reset_replays_exactly() {
    let (s, c, t) = fixture();
    let mut raw = EmbeddedSession::new(s.clone(), c.clone(), 42, CaptureMode::Latest).unwrap();
    let mut env = EmbeddedEnvironment::new(s, c, t, 42).unwrap();
    let initial = env.transition().clone();
    assert_eq!(initial.time_s, 0.0);
    assert_eq!(initial.reward, 0.0);
    assert_eq!(env.contract()["deployable"], false);
    assert_eq!(env.contract()["observations"][0]["unit"], "rad");
    let mut trace = Vec::new();
    for action in [0.2, 0.6, -0.1] {
        raw.set_inputs(&[action]).unwrap();
        raw.advance(80).unwrap();
        let result = env.step(&[action]).unwrap();
        let frame = raw.frame().unwrap();
        assert_eq!(
            result.observations[0],
            frame["joint_positions"][0].as_f64().unwrap()
        );
        assert_eq!(
            result.observations[1],
            frame["joint_velocities"][0].as_f64().unwrap()
        );
        assert_eq!(
            result.observations[3],
            frame["motor_readings"][0]["current_a"].as_f64().unwrap()
        );
        assert_eq!(
            result.reward,
            -((result.observations[0] - 0.3) / 0.2).powi(2) * 2.0 * 0.02
        );
        let mut actual = env.frame().unwrap();
        for key in [
            "done",
            "error",
            "completed_steps",
            "requested_steps",
            "stepping_wall_s",
            "policy_inputs",
        ] {
            actual.as_object_mut().unwrap().remove(key);
        }
        assert_eq!(actual, frame);
        trace.push(result);
    }
    assert!(env.transition().truncated);
    assert!(!env.transition().terminated);
    assert!(env.step(&[0.2]).unwrap_err().contains("reset required"));
    assert_eq!(env.reset(42).unwrap(), initial);
    for (action, expected) in [0.2, 0.6, -0.1].into_iter().zip(trace) {
        assert_eq!(env.step(&[action]).unwrap(), expected);
    }
    let (mut replay, n) =
        EmbeddedSession::prepare_replay(env.recording(), CaptureMode::Latest).unwrap();
    replay.advance(n).unwrap();
    let mut a = env.frame().unwrap();
    let mut b = replay.interactive_frame().unwrap();
    a.as_object_mut().unwrap().remove("stepping_wall_s");
    b.as_object_mut().unwrap().remove("stepping_wall_s");
    assert_eq!(a, b);
}

#[test]
fn invalid_action_preserves_state_and_task_failure_differs_from_timeout() {
    let (s, c, mut t) = fixture();
    t.termination_bounds.push(TerminationBound {
        observation: "angle".into(),
        lower: 0.0,
        upper: 0.0,
    });
    let mut env = EmbeddedEnvironment::new(s, c, t, 0).unwrap();
    let before = env.frame().unwrap();
    for action in [vec![], vec![2.0], vec![f64::NAN]] {
        assert!(env.step(&action).is_err());
        assert_eq!(env.frame().unwrap(), before);
    }
    let result = env.step(&[0.8]).unwrap();
    assert!(result.terminated);
    assert!(!result.truncated);
    assert_eq!(result.termination_reasons.len(), 1);
    let record = serde_json::to_value(env.recording()).unwrap();
    assert!(env.step(&[0.0]).is_err());
    assert_eq!(serde_json::to_value(env.recording()).unwrap(), record);
}

#[test]
fn rejects_clock_ambiguity_missing_sources_and_incompatible_reward_units() {
    let (s, c, t) = fixture();
    for case in 0..8 {
        let mut task = t.clone();
        let mut config = c.clone();
        match case {
            0 => task.period_s = 0.019,
            1 => config.steps = 200,
            2 => task.observation_source = "hardware".into(),
            3 => {
                task.observations[0].source = ObservationSource::CoordinatePosition {
                    coordinate: "missing".into(),
                }
            }
            4 => {
                task.observations[0].source = ObservationSource::ReferencePosition {
                    coordinate: "joint.pivot".into(),
                }
            }
            5 => {
                task.rewards[0].target = Target::Observation {
                    name: "speed".into(),
                }
            }
            6 => task.rewards[0].scale = 0.0,
            7 => task.observations[1].name = "angle".into(),
            _ => unreachable!(),
        }
        assert!(
            EmbeddedEnvironment::new(s.clone(), config, task, 0).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn reward_overflow_latches_failure_instead_of_delivering_a_training_transition() {
    let (s, c, mut t) = fixture();
    t.rewards[0].scale = 1e-300;
    let mut env = EmbeddedEnvironment::new(s, c, t, 0).unwrap();
    assert!(env.step(&[0.2]).unwrap_err().contains("nonfinite reward"));
    assert!(env.error().is_some());
    assert_eq!(env.transition().completed_steps, 0);
    assert_eq!(env.recording().completed_steps, 80);
    assert!(env.step(&[0.2]).unwrap_err().contains("reset required"));
    assert!(env.reset(0).is_ok());
    assert!(env.error().is_none());
}

#[test]
fn environment_replay_preserves_task_scores_and_rejects_changed_recipes() {
    let (s, c, t) = fixture();
    let mut env = EmbeddedEnvironment::new(s, c, t, 71).unwrap();
    let a = env.step(&[0.3]).unwrap();
    let b = env.step(&[-0.2]).unwrap();
    let record = env.episode_recording();
    let (mut replay, actions) = env.prepare_replay(record.clone()).unwrap();
    assert_eq!(actions, vec![vec![0.3], vec![-0.2]]);
    assert_eq!(replay.step(&actions[0]).unwrap(), a);
    assert_eq!(replay.step(&actions[1]).unwrap(), b);
    for i in 0..5 {
        let mut bad = record.clone();
        match i {
            0 => bad.task.rewards[0].scale *= 2.0,
            1 => bad.runtime.input_events[1].at_step += 1,
            2 => bad.runtime.input_events[1].values[0] = 3.0,
            3 => bad.runtime.completed_steps += 1,
            4 => bad.runtime.input_events[1].at_step = 0,
            _ => unreachable!(),
        }
        assert!(env.prepare_replay(bad).is_err());
        assert_eq!(env.transition(), &b);
    }
}

#[test]
fn unchanged_actions_replay_from_initial_values_and_sparse_change_events() {
    let (s, c, t) = fixture();
    let mut env = EmbeddedEnvironment::new(s, c, t, 0).unwrap();
    let mut expected = Vec::new();
    for a in [0.2, 0.4, 0.4] {
        expected.push(env.step(&[a]).unwrap());
    }
    assert_eq!(env.recording().input_events.len(), 1);
    let (mut replay, actions) = env.prepare_replay(env.episode_recording()).unwrap();
    assert_eq!(actions, vec![vec![0.2], vec![0.4], vec![0.4]]);
    for (a, t) in actions.iter().zip(expected) {
        assert_eq!(replay.step(a).unwrap(), t);
    }
    env.reset(0).unwrap();
    env.step(&[0.2]).unwrap();
    assert!(env.recording().input_events.is_empty());
    let (mut replay, actions) = env.prepare_replay(env.episode_recording()).unwrap();
    assert_eq!(replay.step(&actions[0]).unwrap(), *env.transition());
}

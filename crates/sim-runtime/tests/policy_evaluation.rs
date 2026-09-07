use serde_json::json;
use sim_runtime::{
    embedded::Config,
    environment::{EmbeddedEnvironment, Task, TerminationBound},
    policy_evaluation::{evaluate_episode, worst_reward_rate},
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
    let task = serde_json::from_value(json!({"version":1,"observation_source":"ideal_runtime_teacher_only","period_s":0.02,
        "observations":[{"name":"angle","source":{"kind":"coordinate_position","coordinate":"joint.pivot"}}],
        "rewards":[],"termination_bounds":[],"survival_reward_per_s":2.0})).unwrap();
    (scene, config, task)
}

#[test]
fn complete_episode_matches_direct_runtime_and_normalizes_duration() {
    let (s, c, t) = fixture();
    let actions = vec![vec![0.2], vec![0.6], vec![-0.1]];
    let mut env = EmbeddedEnvironment::new(s.clone(), c.clone(), t.clone(), 9).unwrap();
    let mut reward = 0.0;
    for action in &actions {
        reward += env.step(action).unwrap().reward;
    }
    let report = evaluate_episode(s.clone(), c.clone(), t.clone(), &actions, 9);
    assert_eq!(report.score, Some(reward));
    assert_eq!(report.final_transition.as_ref().unwrap(), env.transition());
    assert_eq!(report.completed_actions, 3);
    let mut long = c;
    long.steps = 480;
    let mut lower_reward = t;
    lower_reward.survival_reward_per_s = 1.5;
    let long_report = evaluate_episode(
        s,
        long,
        lower_reward,
        &[actions.clone(), actions].concat(),
        9,
    );
    assert!(long_report.score.unwrap() > report.score.unwrap());
    assert!((worst_reward_rate(&[report, long_report]).unwrap() - 1.5).abs() < 1e-12);
}

#[test]
fn incomplete_actions_and_task_termination_cannot_supply_a_training_score() {
    let (s, c, mut t) = fixture();
    let short = evaluate_episode(s.clone(), c.clone(), t.clone(), &[vec![0.2]], 0);
    assert!(short.score.is_none());
    assert_eq!(short.completed_actions, 0);
    assert!(worst_reward_rate(&[short]).is_err());
    t.termination_bounds.push(TerminationBound {
        observation: "angle".into(),
        lower: 0.0,
        upper: 0.0,
    });
    let failed = evaluate_episode(s, c, t, &vec![vec![0.8]; 3], 0);
    assert_eq!(failed.completed_actions, 1);
    assert!(failed.accrued_reward > 0.0);
    assert!(failed.score.is_none());
    assert!(failed.final_transition.as_ref().unwrap().terminated);
    assert!(worst_reward_rate(&[failed]).is_err());
    assert!(worst_reward_rate(&[]).is_err());
}

#[test]
fn invalid_action_after_a_good_interval_preserves_partial_diagnostics_only() {
    let (s, c, t) = fixture();
    let failed = evaluate_episode(s, c, t, &[vec![0.2], vec![2.0], vec![0.2]], 0);
    assert_eq!(failed.completed_actions, 1);
    assert_eq!(failed.final_transition.unwrap().time_s, 0.02);
    assert_eq!(failed.accrued_reward, 0.04);
    assert!(failed.score.is_none());
    assert!(failed.error.is_some());
}

use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession, MAX_ADVANCE_STEPS},
    session::Scene,
};

#[test]
fn episode_horizon_is_independent_of_bounded_advance_calls() {
    let scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    let short: Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.embedded.json"
    ))
    .unwrap();
    let mut config = short.clone();
    config.steps = 1_920_000;
    let mut long =
        EmbeddedSession::new(scene.clone(), config.clone(), 0, CaptureMode::Latest).unwrap();
    let mut reference = EmbeddedSession::new(scene.clone(), short, 0, CaptureMode::Latest).unwrap();
    long.advance(37).unwrap();
    long.advance(43).unwrap();
    reference.advance(80).unwrap();
    assert_eq!(long.completed_steps(), 80);
    assert_eq!(long.remaining_steps(), 1_919_920);
    let physical = |mut x: serde_json::Value| {
        x.as_object_mut().unwrap().remove("stepping_wall_s");
        x
    };
    assert_eq!(
        physical(long.frame().unwrap()),
        physical(reference.frame().unwrap())
    );
    let before = long.recording();
    assert!(long.advance(MAX_ADVANCE_STEPS + 1).is_err());
    assert_eq!(long.completed_steps(), 80);
    assert_eq!(
        serde_json::to_value(long.recording()).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    let (mut replay, steps) = EmbeddedSession::prepare_replay(before, CaptureMode::Latest).unwrap();
    assert_eq!(steps, 80);
    replay.advance(steps).unwrap();
    assert_eq!(
        physical(replay.frame().unwrap()),
        physical(long.frame().unwrap())
    );
    config.step_s = f64::MAX;
    assert!(EmbeddedSession::new(scene.clone(), config.clone(), 0, CaptureMode::Latest).is_err());
    config.step_s = 0.0001;
    config.steps = 0;
    assert!(EmbeddedSession::new(scene, config, 0, CaptureMode::Latest).is_err());
}

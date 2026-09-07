use sim_runtime::{
    embedded::Config,
    environment::{EmbeddedEnvironment, Task},
    session::Scene,
};
fn fixture() -> (Scene, Config, Task) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/full-robot/browser-online-steps");
    let read = |name: &str| std::fs::read(root.join(name)).unwrap();
    (
        serde_json::from_slice(&read("scene.json")).unwrap(),
        serde_json::from_slice(&read("config.json")).unwrap(),
        serde_json::from_slice(&read("task.json")).unwrap(),
    )
}
#[test]
fn online_initial_reference_is_available_to_task_and_invalid_inputs_preserve_it() {
    let (s, mut c, t) = fixture();
    c.steps = 10;
    let expected: Vec<_> = c
        .motors
        .as_ref()
        .unwrap()
        .servos
        .as_ref()
        .unwrap()
        .iter()
        .map(|s| s.target_rad)
        .collect();
    let mut env = EmbeddedEnvironment::new(s, c, t, 0).unwrap();
    let initial = env.frame().unwrap();
    assert_eq!(
        initial["reference_targets_rad"],
        serde_json::json!(expected)
    );
    assert!(env.step(&[]).is_err());
    assert_eq!(env.frame().unwrap(), initial);
    let action: Vec<_> = env.inputs().iter().map(|c| c.initial).collect();
    env.step(&action).unwrap();
    let frame = env.frame().unwrap();
    assert_eq!(
        frame["policy"]["step_reference"]["reference"]["phase"],
        "hold"
    );
    assert!(
        frame["policy"]["step_reference"]["maximum_marker_error_m"]
            .as_f64()
            .unwrap()
            < 1e-5
    );
    assert_eq!(env.reset(0).unwrap().time_s, 0.);
    assert_eq!(env.frame().unwrap(), initial);
}
#[test]
fn online_reference_rejects_ambiguous_clocks_units_and_invalid_planner_screen() {
    let (scene, config, task) = fixture();
    for case in 0..4 {
        let mut c = config.clone();
        let mut s = scene.clone();
        let reference = c.policy.as_mut().unwrap().step_reference.as_mut().unwrap();
        match case {
            0 => reference.sequence.period_s = 0.01,
            1 => reference.minimum_planned_support_force_n = Some(-1.),
            2 => {
                s.controller
                    .as_mut()
                    .unwrap()
                    .inputs
                    .last_mut()
                    .unwrap()
                    .kind = sim_core::QuantityKind::Length
            }
            3 => reference.command_channels[1] = reference.command_channels[0].clone(),
            _ => unreachable!(),
        }
        assert!(
            EmbeddedEnvironment::new(s, c, task.clone(), 0).is_err(),
            "case {case}"
        );
    }
}

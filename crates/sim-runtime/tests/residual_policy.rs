use serde_json::Value;
use sim_runtime::{embedded::Config, environment::{EmbeddedEnvironment, Task}, session::Scene};

#[test]
fn each_robot_residual_reaches_only_its_named_target_and_preserves_input_bounds() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/full-robot/browser-residual-policy");
    let read = |name: &str| std::fs::read(root.join(name)).unwrap();
    let scene: Scene = serde_json::from_slice(&read("scene.json")).unwrap();
    let config: Config = serde_json::from_slice(&read("short.config.json")).unwrap();
    let task: Task = serde_json::from_slice(&read("task.json")).unwrap();
    let spec: Value = serde_json::from_slice(&read("learning.json")).unwrap();
    let mut env = EmbeddedEnvironment::new(scene, config, task, 0).unwrap();
    let neutral: Vec<_> = env.inputs().iter().map(|c| c.initial).collect();
    env.step(&neutral).unwrap();
    let baseline = env.frame().unwrap();
    for (i, binding) in spec["policy_action_bindings"].as_array().unwrap().iter().enumerate() {
        env.reset(0).unwrap();
        let index = env.inputs().iter().position(|c| c.name == binding["input"].as_str().unwrap()).unwrap();
        let mut action = neutral.clone();
        action[index] = if i % 2 == 0 { 0.001 } else { -0.001 };
        env.step(&action).unwrap();
        let frame = env.frame().unwrap();
        for (j, observation) in env.task().observations.iter().enumerate() {
            if let sim_runtime::environment::ObservationSource::FloorForce { link, axis } = &observation.source {
                let index = frame["poses"].as_array().unwrap().iter().position(|p| p["name"] == link.as_str()).unwrap();
                let axis = match axis { sim_runtime::environment::Axis::X => 0, sim_runtime::environment::Axis::Y => 1, sim_runtime::environment::Axis::Z => 2 };
                let force: f64 = frame["contacts"].as_array().unwrap().iter()
                    .filter(|c| c["link"].as_u64().unwrap() as usize == index && c["other"].is_null())
                    .map(|c| c["force_n"][axis].as_f64().unwrap()).sum();
                assert_eq!(env.transition().observations[j], force);
            }
        }
        for (name, value) in frame["policy"]["targets"].as_object().unwrap() {
            let expected = baseline["policy"]["targets"][name].as_f64().unwrap()
                + if name == binding["target"].as_str().unwrap() { action[index] } else { 0.0 };
            assert!((value.as_f64().unwrap() - expected).abs() < 1e-14, "motor {i}: {name}");
        }
        assert_ne!(frame["joint_positions"], baseline["joint_positions"], "residual {i} must affect live physics");
        env.reset(0).unwrap();
        let before = env.frame().unwrap();
        action[index] = 0.010001;
        assert!(env.step(&action).is_err());
        assert_eq!(env.frame().unwrap(), before);
        assert!(env.error().is_none());
    }
}

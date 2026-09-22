use serde_json::{Value, json};
use sim_runtime::{
    experiment::{ExperimentSpec, Objective},
    experiment_variants::*,
};
fn fixture() -> ExperimentSpec {
    let robot: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/baseline/robot.simrobot.json"
    ))
    .unwrap();
    let mut inputs: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/velocity-controller.inputs.json"
    ))
    .unwrap();
    inputs.as_array_mut().unwrap().push(
        json!({"name":"sequence","kind":"Dimensionless","lower":0.,"upper":100.,"initial":0.}),
    );
    let scene=serde_json::from_value(json!({"version":1,"robot":robot,"options":{"contact":false,"flex":false},"period_s":0.003,"duration_s":0.03,"controller":{"inputs":inputs,"parameters":{"period_s":0.003,"initial_left":0.01,"initial_right":-0.02},"sources":{"entry":"velocity-controller.rhai","files":{"velocity-controller.rhai":include_str!("../../../examples/wheeled-robot/velocity-controller.rhai")}}}})).unwrap();
    ExperimentSpec {
        version: 1,
        scene,
        config: serde_json::from_str(include_str!(
            "../../../examples/wheeled-robot/imu-policy.config.json"
        ))
        .unwrap(),
        task: serde_json::from_str(include_str!(
            "../../../examples/wheeled-robot/imu-policy.task.json"
        ))
        .unwrap(),
        parameterization: serde_json::from_value(
            json!({"version":1,"space":{"parameters":[]},"trajectories":[],"commands":[]}),
        )
        .unwrap(),
        source_actions: (0..10).map(|i| vec![0.01, -0.02, i as f64]).collect(),
        baseline: Default::default(),
        seed: 3,
        objective: Objective::RewardRate,
    }
}
fn case() -> Case {
    Case {
        name: "lower-voltage".into(),
        rationale: "Explicit test sensitivity".into(),
        duration_s: 0.06,
        tail: Some(HeldTail {
            sequence: vec![SequenceIncrement {
                input: "sequence".into(),
                increment: 1.,
            }],
        }),
        edits: vec![Edit {
            target: Target::Config,
            pointer: "/motors/servos/0/supply_voltage_v".into(),
            unit: "V".into(),
            change: Change::Set { value: 5. },
        }],
    }
}
#[test]
fn preserves_source_prefix_and_records_longer_physical_case() {
    let source = fixture();
    let before = serde_json::to_value(&source).unwrap();
    let output = prepare(&source, &case()).unwrap();
    assert_eq!(before, serde_json::to_value(&source).unwrap());
    assert_eq!(&output.spec.source_actions[..10], source.source_actions);
    assert_eq!(
        output.spec.source_actions.last().unwrap(),
        &vec![0.01, -0.02, 19.]
    );
    assert_eq!(output.spec.config.steps, 20);
    assert_eq!(output.spec.config.step_s, source.config.step_s);
    assert_eq!(output.added_action_intervals, 10);
    assert_eq!(output.edits[0].before, 6.);
    assert_eq!(output.edits[0].after, 5.);
    assert_ne!(output.source_spec_blake3, output.prepared_spec_blake3);
    sim_runtime::experiment::Experiment::bind(output.spec).unwrap();
}
#[test]
fn physical_edits_retain_original_cad_input_receipt() {
    let source = fixture();
    let mut case = case();
    case.edits = vec![Edit {
        target: Target::Robot,
        pointer: "/motors/0/electrical/resistance".into(),
        unit: "ohm".into(),
        change: Change::Scale { factor: 1.1 },
    }];
    let output = prepare(&source, &case).unwrap();
    assert!(
        (output.spec.scene.robot.motors[0].electrical.resistance
            - source.scene.robot.motors[0].electrical.resistance * 1.1)
            .abs()
            < 1e-12
    );
    let encoded = serde_json::to_value(&output.spec).unwrap();
    let reread: ExperimentSpec = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(encoded, serde_json::to_value(&reread).unwrap());
    assert_eq!(source.scene.robot.source, reread.scene.robot.source);
    sim_runtime::experiment::Experiment::bind(reread).unwrap();
}
#[test]
fn refuses_implicit_inputs_missing_fields_and_clock_edits() {
    let source = fixture();
    let mut c = case();
    c.tail = None;
    assert!(prepare(&source, &c).is_err());
    let mut c = case();
    c.duration_s = 0.061;
    assert!(prepare(&source, &c).is_err());
    let mut c = case();
    c.duration_s = 0.015;
    assert!(prepare(&source, &c).is_err());
    let mut c = case();
    c.edits[0].pointer = "/step_s".into();
    assert!(prepare(&source, &c).is_err());
    let mut c = case();
    c.edits[0].pointer = "/missing".into();
    assert!(prepare(&source, &c).is_err());
    let mut c = case();
    c.tail.as_mut().unwrap().sequence[0].increment = 100.;
    assert!(prepare(&source, &c).is_err());
    let mut c = case();
    c.edits.push(c.edits[0].clone());
    assert!(prepare(&source, &c).is_err());
}

#[test]
fn timestep_refinement_changes_only_physics_clock_and_preserves_input_schedule() {
    let source = fixture();
    let r = refine_timestep(&source, 2).unwrap();
    assert_eq!(r.spec.config.step_s, 0.0015);
    assert_eq!(r.spec.config.steps, 20);
    assert_eq!(r.spec.config.report_every, 2);
    let mut restored = r.spec.clone();
    restored.config.step_s = source.config.step_s;
    restored.config.steps = source.config.steps;
    restored.config.report_every = source.config.report_every;
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(&source).unwrap()
    );
    sim_runtime::experiment::Experiment::bind(r.spec).unwrap();
    assert!(refine_timestep(&source, 1).is_err());
    assert!(refine_timestep(&source, usize::MAX).is_err());
    let mut invalid = source.clone();
    invalid.config.report_every = 3;
    assert!(refine_timestep(&invalid, 2).is_err());
}

use serde_json::{Value, json};
use sim_domain_robot::motor::MotorDynamics;
use sim_runtime::{
    experiment::{ExperimentSpec, Objective},
    exploration::*,
    fidelity::{EnvironmentCapture, Provenance},
};
fn recipe() -> Recipe {
    let robot: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/baseline/robot.simrobot.json"
    ))
    .unwrap();
    let inputs: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/velocity-controller.inputs.json"
    ))
    .unwrap();
    let scene=serde_json::from_value(json!({"version":1,"robot":robot,"options":{"contact":false,"flex":false},"period_s":0.003,"duration_s":0.03,"controller":{"inputs":inputs,"parameters":{"period_s":0.003,"initial_left":0.01,"initial_right":-0.02},"sources":{"entry":"velocity-controller.rhai","files":{"velocity-controller.rhai":include_str!("../../../examples/wheeled-robot/velocity-controller.rhai")}}}})).unwrap();
    let config = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.config.json"
    ))
    .unwrap();
    let task = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.task.json"
    ))
    .unwrap();
    Recipe {
        version: 1,
        detailed: ExperimentSpec {
            version: 1,
            scene,
            config,
            task,
            parameterization: serde_json::from_value(
                json!({"version":1,"space":{"parameters":[]},"trajectories":[],"commands":[]}),
            )
            .unwrap(),
            source_actions: vec![vec![0.01, -0.02]; 10],
            baseline: Default::default(),
            seed: 3,
            objective: Objective::RewardRate,
        },
        profile: Profile {
            version: 1,
            name: "test-only-loose-budgets".into(),
            motor_dynamics: MotorDynamics::QuasistaticWinding,
            step_multiplier: 1,
            linearized_mechanism_probes: false,
            predict_velocity_seed: false,
            broyden_updates: false,
            absolute_tolerances: [
                "rad", "rad/s", "m", "m/s", "m/s²", "1", "s", "N", "A", "V", "W", "N·m", "J",
            ]
            .into_iter()
            .map(|u| (u.into(), 1e6))
            .collect(),
            minimum_speedup: 1.1,
            task_level: None,
        },
    }
}
fn provenance() -> Provenance {
    Provenance {
        capture_reference: "in-memory test".into(),
        source_reference: "test source".into(),
        executable_reference: "cargo test".into(),
        host_reference: "test host".into(),
        timing_scope: "synthetic timing; not a benchmark".into(),
    }
}
fn capture(spec: &ExperimentSpec, time: f64) -> EnvironmentCapture {
    let mut s = CaptureSession::new(spec).unwrap();
    while !s.done() {
        s.advance().unwrap();
    }
    s.capture(time)
}
fn q(r: &Recipe, a: &EnvironmentCapture, b: &EnvironmentCapture) -> Qualification {
    qualify(r, a, b, provenance(), provenance()).unwrap()
}
#[test]
fn profile_preserves_every_physical_and_controller_field() {
    let r = recipe();
    let p = r.prepare().unwrap();
    assert_eq!(p.changes.len(), 1);
    assert_eq!(
        p.changes[0].difference.path,
        "/scene/options/motor_dynamics"
    );
    assert_eq!(json!(p.detailed.scene.robot), json!(p.reduced.scene.robot));
    assert_eq!(
        json!(p.detailed.scene.controller),
        json!(p.reduced.scene.controller)
    );
    assert_eq!(json!(p.detailed.config), json!(p.reduced.config));
    let mut restored = p.reduced;
    restored.scene.options.motor_dynamics = MotorDynamics::Detailed;
    assert_eq!(json!(restored), json!(r.detailed));
    let mut r = r;
    r.detailed.config.step_s /= 2.;
    r.detailed.config.steps *= 2;
    r.detailed.config.report_every *= 2;
    r.profile.step_multiplier = 2;
    let p = r.prepare().unwrap();
    assert_eq!(p.reduced.config.steps, 10);
    assert_eq!(p.changes.len(), 4);
    r.profile.motor_dynamics = MotorDynamics::Detailed;
    assert_eq!(r.prepare().unwrap().changes.len(), 3);
    r.profile.step_multiplier=1;
    r.profile.linearized_mechanism_probes=true;
    let p=r.prepare().unwrap();
    assert!(p.reduced.config.implicit.as_ref().unwrap().linearized_jacobian_probes);
    assert_eq!(p.reduced.config.step_s,p.detailed.config.step_s);
    r.detailed=p.reduced;
    assert!(r.prepare().is_err());
}
#[test]
fn refuses_implicit_reductions_misaligned_clocks_and_bad_references() {
    let mut r = recipe();
    r.profile.motor_dynamics = MotorDynamics::QuasistaticRotor;
    assert!(r.prepare().is_err());
    let mut r = recipe();
    r.profile.step_multiplier = 2;
    assert!(r.prepare().is_err());
    let mut r = recipe();
    r.detailed.source_actions.pop();
    assert!(r.prepare().is_err());
    let mut r = recipe();
    r.detailed.source_actions[0][0] = 1e99;
    assert!(r.prepare().is_err());
    let mut r = recipe();
    r.detailed.scene.options.motor_dynamics = MotorDynamics::Quasistatic;
    assert!(r.prepare().is_err());
}
#[test]
fn production_capture_qualification_and_journal_use_one_runtime() {
    let r = recipe();
    let p = r.prepare().unwrap();
    let a = capture(&p.detailed, 2.);
    let b = capture(&p.reduced, 1.);
    let result = q(&r, &a, &b);
    assert!(result.qualified, "{:?}", result.rejection_reasons);
    assert!(!result.electrical.is_empty());
    let journal = qualified_journal(&r, &a, &b, provenance(), provenance()).unwrap();
    assert!(journal.trials.is_empty());
    assert_eq!(
        journal.experiment.spec.scene.options.motor_dynamics,
        MotorDynamics::QuasistaticWinding
    );
    let finalist = detailed_candidate(&r, Default::default()).unwrap();
    assert!(finalist.scene.options.motor_dynamics.is_detailed());
    let replay = capture(&p.reduced, 1.);
    let mut compare = r.clone();
    compare
        .profile
        .absolute_tolerances
        .values_mut()
        .for_each(|v| *v = 0.);
    // Actual repeat output, excluding wall times, is deterministic.
    for (x, y) in b.frames.iter().zip(&replay.frames) {
        let mut x = x.clone();
        let mut y = y.clone();
        x.as_object_mut().unwrap().remove("stepping_wall_s");
        y.as_object_mut().unwrap().remove("stepping_wall_s");
        assert_eq!(x, y);
    }
}
#[test]
fn failed_incomplete_slow_or_inaccurate_captures_never_unlock_search() {
    let r = recipe();
    let p = r.prepare().unwrap();
    let a = capture(&p.detailed, 2.);
    let mut b = capture(&p.reduced, 1.);
    b.wall_s = 3.;
    assert!(!q(&r, &a, &b).qualified);
    b.wall_s = 1.;
    let mut strict = r.clone();
    strict
        .profile
        .absolute_tolerances
        .values_mut()
        .for_each(|v| *v = 0.);
    assert!(!q(&strict, &a, &b).qualified);
    let mut session = CaptureSession::new(&p.reduced).unwrap();
    session.advance().unwrap();
    let partial = session.capture(0.1);
    assert!(!q(&r, &a, &partial).qualified);
    assert!(qualified_journal(&r, &a, &partial, provenance(), provenance()).is_err());
    b.completed = false;
    assert!(qualify(&r, &a, &b, provenance(), provenance()).is_err());
}
#[test]
fn recipe_runtime_actions_and_observation_tampering_are_rejected() {
    let r = recipe();
    let p = r.prepare().unwrap();
    let a = capture(&p.detailed, 2.);
    let b = capture(&p.reduced, 1.);
    let mut wrong = b.clone();
    wrong.recording.scene.robot.gravity[2] = -7.;
    assert!(qualify(&r, &a, &wrong, provenance(), provenance()).is_err());
    let mut wrong = b.clone();
    wrong.frames[0]["motor_readings"] = json!([]);
    assert!(qualify(&r, &a, &wrong, provenance(), provenance()).is_err());
    let mut wrong = b.clone();
    wrong.recording.input_events[0].values[0] += 0.01;
    assert!(qualify(&r, &a, &wrong, provenance(), provenance()).is_err());
    let mut wrong = b;
    wrong
        .recording
        .runtime_identity
        .as_mut()
        .unwrap()
        .library_source_blake3 = "0".repeat(64);
    assert!(qualify(&r, &a, &wrong, provenance(), provenance()).is_err());
}

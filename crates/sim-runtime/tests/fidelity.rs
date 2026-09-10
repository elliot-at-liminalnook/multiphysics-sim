use serde_json::{Value, json};
use sim_runtime::{
    embedded::Config,
    environment::{EmbeddedEnvironment, Task},
    fidelity::*,
    session::Scene,
};

fn fixture(step: f64) -> EnvironmentCapture {
    let mut config: Config = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.config.json"
    ))
    .unwrap();
    config.step_s = step;
    config.steps = (0.03 / step).round() as usize;
    let task: Task = serde_json::from_str(include_str!(
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
    let scene: Scene = serde_json::from_value(json!({"version":1,"robot":robot,"options":{"contact":false,"flex":false},
        "period_s":0.003,"duration_s":0.03,"controller":{"inputs":inputs,
        "parameters":{"period_s":0.003,"initial_left":0.01,"initial_right":-0.02},
        "sources":{"entry":"velocity-controller.rhai","files":{"velocity-controller.rhai":include_str!("../../../examples/wheeled-robot/velocity-controller.rhai")}}}})).unwrap();
    let mut env = EmbeddedEnvironment::new(scene, config, task.clone(), 3).unwrap();
    let mut frames = vec![env.frame().unwrap()];
    let mut transitions = vec![env.transition().clone()];
    for i in 0..10 {
        transitions.push(env.step(&[0.03 * i as f64, 0.1 - 0.02 * i as f64]).unwrap());
        frames.push(env.frame().unwrap());
    }
    EnvironmentCapture {
        version: 1,
        kind: "sampled_environment_capture".into(),
        completed: true,
        error: None,
        recording: env.recording(),
        task,
        metadata: env.metadata(),
        frames,
        transitions,
        wall_s: 1.,
    }
}
fn plan() -> ComparisonPlan {
    let provenance = Provenance {
        capture_reference: "test in-memory capture".into(),
        source_reference: "current test source".into(),
        executable_reference: "current cargo test binary".into(),
        host_reference: "test host".into(),
        timing_scope: "synthetic one second; no performance measurement".into(),
    };
    ComparisonPlan {
        version: 1,
        reference: provenance.clone(),
        candidate: provenance,
        changes: vec![],
        absolute_tolerances: ["rad", "rad/s", "m", "m/s", "m/s²", "1", "s", "N"]
            .into_iter()
            .map(|u| (u.into(), 0.))
            .collect(),
    }
}
fn declare(a: &EnvironmentCapture, b: &EnvironmentCapture, p: &mut ComparisonPlan) {
    p.changes = ExecutionContext::new(&a.recording, &a.task)
        .differences(&ExecutionContext::new(&b.recording, &b.task))
        .unwrap()
        .into_iter()
        .map(|difference| DeclaredChange {
            difference,
            reason: "explicit test perturbation".into(),
        })
        .collect();
}
fn error(a: &EnvironmentCapture, b: &EnvironmentCapture, p: &ComparisonPlan) -> String {
    match compare(a, b, p) {
        Err(e) => e,
        Ok(_) => panic!("comparison unexpectedly accepted invalid inputs"),
    }
}

#[test]
fn identical_capture_is_exact_and_preserves_outcome_cost_and_resolved_context() {
    let a = fixture(0.003);
    let p = plan();
    let r = compare(&a, &a, &p).unwrap();
    assert_eq!(r.compared_frames, 11);
    assert_eq!(r.matched_input_duration_s, 0.03);
    assert!(r.trajectory_within_tolerances && r.categorical_outcomes_match);
    assert!(r.channels.len() > 80);
    assert!(
        r.channels
            .values()
            .all(|c| c.maximum_absolute == 0. && c.rms == 0.)
    );
    assert!(r.reference.eligible_score.is_some());
    assert_eq!(r.reference.simulated_s_per_wall_s, 0.03);
    assert_eq!(json!(r.reference_context.scene), json!(a.recording.scene));
    let encoded = serde_json::to_vec(&r).unwrap();
    let decoded: ComparisonReport = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.channels.len(), r.channels.len());
}

#[test]
fn timestep_comparison_requires_exact_declared_edits_and_matches_actions_in_seconds() {
    let a = fixture(0.003);
    let b = fixture(0.0015);
    let mut p = plan();
    assert!(error(&a, &b, &p).contains("undeclared"));
    declare(&a, &b, &mut p);
    assert_eq!(
        p.changes
            .iter()
            .map(|c| c.difference.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/config/step_s", "/config/steps"]
    );
    let p: ComparisonPlan = serde_json::from_value(json!(p)).unwrap();
    let r = compare(&a, &b, &p).unwrap();
    assert_eq!(r.compared_frames, 11);
    assert!(r.categorical_outcomes_match);
    assert!(!r.trajectory_within_tolerances);
    assert!(r.channels.values().any(|c| c.maximum_absolute > 0.));
    let mut wrong = p.clone();
    wrong.changes[0].difference.candidate = ContextValue::Present(json!(0.001));
    assert!(error(&a, &b, &wrong).contains("mismatched"));
    assert!(error(&a, &a, &p).contains("unused"));
}

#[test]
fn context_covers_world_actuators_initial_conditions_seed_policy_and_missing_values() {
    let a = fixture(0.003);
    let mut b = a.clone();
    let mut p = plan();
    b.recording.scene.robot.gravity[2] = -8.;
    assert!(error(&a, &b, &p).contains("/scene/robot/gravity/2"));
    b = a.clone();
    b.recording.config.initial_base_rotation_vector_rad = Some([0., 0., 0.]);
    declare(&a, &b, &mut p);
    assert_eq!(p.changes[0].difference.reference, ContextValue::Missing);
    let restored: ComparisonPlan = serde_json::from_value(json!(p)).unwrap();
    assert_eq!(restored.changes[0].difference, p.changes[0].difference);
    b = a.clone();
    b.recording.config.initial_coordinates = Some(vec![0.; 3]);
    declare(&a, &b, &mut p);
    assert_eq!(
        p.changes[0].difference.reference,
        ContextValue::Present(Value::Null)
    );
    let restored: ComparisonPlan = serde_json::from_value(json!(p)).unwrap();
    assert_eq!(restored.changes[0].difference, p.changes[0].difference);
    b = a.clone();
    b.recording.seed += 1;
    declare(&a, &b, &mut p);
    assert!(error(&a, &b, &p).contains("identical task, seed"));
    b = a.clone();
    b.recording
        .config
        .motors
        .as_mut()
        .unwrap()
        .servos
        .as_mut()
        .unwrap()[0]
        .target_rad += 0.1;
    declare(&a, &b, &mut p);
    assert!(error(&a, &b, &p).contains("context mismatch"));
}

#[test]
fn rejects_unmatched_actions_frames_metadata_and_unexplained_terminal_events() {
    let a = fixture(0.003);
    let p = plan();
    let mut b = a.clone();
    b.recording.input_events[0].values[0] += 0.01;
    assert!(error(&a, &b, &p).contains("endpoint controller inputs"));
    for f in b.frames.iter_mut().skip(1) {
        f.as_object_mut().unwrap().remove("policy_inputs");
    }
    assert!(error(&a, &b, &p).contains("identical held actions"));
    b = a.clone();
    b.frames[2]["time_s"] = json!(0.0061);
    assert!(error(&a, &b, &p).contains("aligned"));
    b = a.clone();
    b.metadata["frame_coordinates"][0]["position_unit"] = json!("m");
    assert!(error(&a, &b, &p).contains("coordinate identities"));
    b = a.clone();
    let mut e = b.recording.input_events[0].clone();
    e.at_step = 10;
    b.recording.input_events.push(e);
    assert!(error(&a, &b, &p).contains("uncommitted event"));
    let mut missing = p.clone();
    missing.absolute_tolerances.remove("m");
    assert!(error(&a, &a, &missing).contains("missing fidelity tolerance"));
}

#[test]
fn known_position_and_contact_errors_are_measured_by_name_in_si_units() {
    let a = fixture(0.003);
    let mut b = a.clone();
    let mut p = plan();
    let x = b.frames[5]["poses"][0]["position_m"][0].as_f64().unwrap();
    b.frames[5]["poses"][0]["position_m"][0] = json!(x + 0.01);
    b.frames[5]["contacts"] =
        json!([{"link":0,"force_n":[1.,2.,3.]},{"link":0,"force_n":[2.,-2.,0.]}]);
    let r = compare(&a, &b, &p).unwrap();
    let c = &r.channels["link/chassis/position/0"];
    assert!((c.maximum_absolute - 0.01).abs() < 1e-15);
    assert!((c.rms - 0.01 / 11f64.sqrt()).abs() < 1e-15);
    assert_eq!(c.worst_time_s, 0.015);
    assert_eq!(c.unit, "m");
    assert_eq!(r.channels["contact/chassis/force/0"].maximum_absolute, 3.);
    assert_eq!(r.channels["contact/chassis/force/1"].maximum_absolute, 0.);
    assert!(!r.trajectory_within_tolerances);
    p.absolute_tolerances.insert("m".into(), 0.011);
    p.absolute_tolerances.insert("N".into(), 3.);
    assert!(compare(&a, &b, &p).unwrap().trajectory_within_tolerances);
    // Named link matching, not positional zip; remap indexed contact ownership.
    b.frames[5]["poses"].as_array_mut().unwrap().reverse();
    let last = b.frames[5]["poses"].as_array().unwrap().len() - 1;
    for c in b.frames[5]["contacts"].as_array_mut().unwrap() {
        c["link"] = json!(last);
    }
    assert_eq!(
        compare(&a, &b, &p).unwrap().channels["contact/chassis/force/0"].maximum_absolute,
        3.
    );
}

#[test]
fn failed_and_incomplete_prefixes_never_gain_an_eligible_score() {
    let a = fixture(0.003);
    let mut b = a.clone();
    let p = plan();
    b.frames.truncate(6);
    b.transitions.truncate(6);
    b.recording.completed_steps = 5;
    b.recording.input_events.retain(|e| e.at_step < 5);
    b.completed = false;
    let r = compare(&a, &b, &p).unwrap();
    assert_eq!(r.matched_input_duration_s, 0.015);
    assert!(r.candidate.eligible_score.is_none());
    assert!(!r.categorical_outcomes_match);
    b.error = Some("test solver failure".into());
    b.recording.failure = b.error.clone();
    let mut e = b.recording.input_events[0].clone();
    e.at_step = 5;
    b.recording.input_events.push(e);
    let r = compare(&a, &b, &p).unwrap();
    assert!(r.candidate.eligible_score.is_none());
    assert_eq!(
        r.candidate.runtime_failure.as_deref(),
        Some("test solver failure")
    );
    b.completed = true;
    assert!(error(&a, &b, &p).contains("completion metadata"));
}

#[test]
fn solver_failure_can_commit_physics_beyond_the_last_observed_task_endpoint() {
    let a = fixture(0.0015);
    let mut b = a.clone();
    let p = plan();
    b.frames.truncate(6);
    b.transitions.truncate(6);
    b.completed = false;
    b.recording.completed_steps = 11;
    b.recording.input_events.retain(|e| e.at_step <= 10);
    b.error = Some("failure inside an action interval".into());
    b.recording.failure = b.error.clone();
    let r = compare(&a, &b, &p).unwrap();
    assert_eq!(r.candidate.observed_s, 0.015);
    assert_eq!(r.candidate.simulated_s, 0.0165);
    assert_eq!(r.candidate.unobserved_committed_steps, 1);
    assert!(r.candidate.eligible_score.is_none());
    // The first command may fail before any complete observation interval.
    b.frames.truncate(1);
    b.transitions.truncate(1);
    b.recording.completed_steps = 1;
    b.recording.input_events.truncate(1);
    let r = compare(&a, &b, &p).unwrap();
    assert_eq!(r.compared_frames, 1);
    assert_eq!(r.matched_input_duration_s, 0.);
    assert_eq!(r.candidate.unobserved_committed_steps, 1);
    assert!(r.candidate.eligible_score.is_none());
}
